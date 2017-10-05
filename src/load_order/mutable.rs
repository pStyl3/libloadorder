/*
 * This file is part of libloadorder
 *
 * Copyright (C) 2017 Oliver Hamlet
 *
 * libloadorder is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * libloadorder is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with libloadorder. If not, see <http://www.gnu.org/licenses/>.
 */

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::mem;
use std::path::Path;

use encoding::all::WINDOWS_1252;
use encoding::{DecoderTrap, Encoding};
use rayon::prelude::*;
use walkdir::WalkDir;

use enums::Error;
use game_settings::GameSettings;
use load_order::find_first_non_master_position;
use load_order::readable::ReadableLoadOrder;
use plugin::Plugin;

pub const MAX_ACTIVE_PLUGINS: usize = 255;

pub trait MutableLoadOrder: ReadableLoadOrder {
    fn plugins_mut(&mut self) -> &mut Vec<Plugin>;

    fn insert_position(&self, plugin: &Plugin) -> Option<usize>;

    fn insert(&mut self, plugin: Plugin) -> usize {
        match self.insert_position(&plugin) {
            Some(position) => {
                self.plugins_mut().insert(position, plugin);
                position
            }
            None => {
                self.plugins_mut().push(plugin);
                self.plugins().len() - 1
            }
        }
    }

    fn add_to_load_order(&mut self, plugin_name: &str) -> Result<usize, Error> {
        let plugin = Plugin::new(plugin_name, self.game_settings())?;

        Ok(self.insert(plugin))
    }

    fn count_active_plugins(&self) -> usize {
        self.plugins().iter().filter(|p| p.is_active()).count()
    }

    fn load_plugins_if_valid(&self, filenames: Vec<String>) -> Vec<Plugin> {
        let game_settings = self.game_settings();
        filenames
            .par_iter()
            .filter_map(|f| Plugin::new(&f, game_settings).ok())
            .collect()
    }

    fn find_plugins_in_dir(&self) -> Vec<String> {
        WalkDir::new(self.game_settings().plugins_directory())
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.file_name().to_str().and_then(|f| Some(f.to_owned())))
            .collect()
    }

    fn find_plugins_in_dir_sorted(&self) -> Vec<String> {
        let mut filenames = self.find_plugins_in_dir();
        filenames.par_sort();

        filenames
    }

    fn load_unique_plugins(
        &mut self,
        plugin_name_tuples: Vec<(String, bool)>,
        installed_filenames: Vec<String>,
    ) {
        let plugins: Vec<Plugin> = {
            let game_settings = self.game_settings();

            remove_duplicates_icase(plugin_name_tuples, installed_filenames)
                .into_par_iter()
                .filter_map(|(filename, active)| {
                    Plugin::with_active(&filename, game_settings, active).ok()
                })
                .collect()
        };

        for plugin in plugins {
            self.insert(plugin);
        }
    }

    fn activate_unvalidated(&mut self, filename: &str) -> Result<(), Error> {
        let index = {
            let index = self.index_of(filename);
            if index.is_none() && Plugin::is_valid(&filename, self.game_settings()) {
                Some(self.add_to_load_order(filename)?)
            } else {
                index
            }
        };

        if let Some(x) = index {
            self.plugins_mut()[x].activate()?;
        }

        Ok(())
    }

    fn add_implicitly_active_plugins(&mut self) -> Result<(), Error> {
        for filename in self.game_settings().implicitly_active_plugins() {
            if self.is_active(filename) {
                continue;
            }

            self.activate_unvalidated(filename)?;
        }

        Ok(())
    }

    fn deactivate_excess_plugins(&mut self) {
        let implicitly_active_plugins = self.game_settings().implicitly_active_plugins();
        let mut count = self.count_active_plugins();

        for plugin in self.plugins_mut().iter_mut().rev() {
            if count <= MAX_ACTIVE_PLUGINS {
                break;
            }
            if plugin.is_active() &&
                !implicitly_active_plugins.iter().any(
                    |i| plugin.name_matches(i),
                )
            {
                plugin.deactivate();
                count -= 1;
            }
        }
    }

    fn move_or_insert_plugin_with_index(
        &mut self,
        plugin_name: &str,
        position: usize,
    ) -> Result<(), Error> {
        if let Some(x) = self.index_of(plugin_name) {
            if x == position {
                return Ok(());
            }
        }

        let plugin = get_plugin_to_insert_at(self, plugin_name, position)?;

        if position >= self.plugins().len() {
            self.plugins_mut().push(plugin);
        } else {
            self.plugins_mut().insert(position, plugin);
        }

        Ok(())
    }

    fn find_plugin_mut<'a>(&'a mut self, plugin_name: &str) -> Option<&'a mut Plugin> {
        self.plugins_mut().iter_mut().find(
            |p| p.name_matches(plugin_name),
        )
    }

    fn deactivate_all(&mut self) {
        for plugin in self.plugins_mut() {
            plugin.deactivate();
        }
    }
}

pub fn replace_plugins<T>(load_order: &mut T, plugin_names: &[&str]) -> Result<(), Error>
where
    T: MutableLoadOrder + Sync,
{
    validate_plugin_names(plugin_names, load_order.game_settings())?;

    let mut plugins = map_to_plugins(load_order, plugin_names)?;

    if !is_partitioned_by_master_flag(&plugins) {
        return Err(Error::NonMasterBeforeMaster);
    }

    mem::swap(&mut plugins, load_order.plugins_mut());

    add_missing_plugins(load_order);

    load_order.add_implicitly_active_plugins()
}

pub fn load_active_plugins<T, F>(load_order: &mut T, line_mapper: F) -> Result<(), Error>
where
    T: MutableLoadOrder + Sync,
    F: Fn(&str) -> Option<String> + Send + Sync,
{
    load_order.deactivate_all();

    let plugin_names = read_plugin_names(
        load_order.game_settings().active_plugins_file(),
        line_mapper,
    )?;

    let plugin_indices: Vec<usize> = plugin_names
        .par_iter()
        .filter_map(|p| load_order.index_of(p))
        .collect();

    for index in plugin_indices {
        load_order.plugins_mut()[index].activate()?;
    }

    Ok(())
}

pub fn read_plugin_names<F, T>(file_path: &Path, line_mapper: F) -> Result<Vec<T>, Error>
where
    F: Fn(&str) -> Option<T> + Send + Sync,
    T: Send,
{
    if !file_path.exists() {
        return Ok(Vec::new());
    }

    let mut content: Vec<u8> = Vec::new();
    let mut file = File::open(file_path)?;
    file.read_to_end(&mut content)?;

    let content = WINDOWS_1252.decode(&content, DecoderTrap::Strict).map_err(
        |e| {
            Error::DecodeError(e)
        },
    )?;

    Ok(content.par_lines().filter_map(line_mapper).collect())
}

pub fn plugin_line_mapper(line: &str) -> Option<String> {
    if line.is_empty() || line.starts_with('#') {
        None
    } else {
        Some(line.to_owned())
    }
}

fn add_missing_plugins<T>(load_order: &mut T)
where
    T: MutableLoadOrder + Sync,
{
    let filenames: Vec<String> = load_order
        .find_plugins_in_dir_sorted()
        .into_par_iter()
        .filter(|f| {
            !load_order.game_settings().is_implicitly_active(f) && load_order.index_of(f).is_none()
        })
        .collect();

    for plugin in load_order.load_plugins_if_valid(filenames) {
        load_order.insert(plugin);
    }
}

fn remove_duplicates_icase(
    plugin_tuples: Vec<(String, bool)>,
    filenames: Vec<String>,
) -> Vec<(String, bool)> {
    let mut set: HashSet<String> = HashSet::with_capacity(filenames.len());

    let mut unique_tuples: Vec<(String, bool)> = plugin_tuples
        .into_iter()
        .rev()
        .filter(|&(ref string, _)| set.insert(string.to_lowercase()))
        .collect();

    unique_tuples.reverse();

    let unique_file_tuples_iter = filenames
        .into_iter()
        .filter(|ref string| set.insert(string.to_lowercase()))
        .map(|f| (f, false));

    unique_tuples.extend(unique_file_tuples_iter);

    unique_tuples
}

fn validate_index<T: MutableLoadOrder + ?Sized>(
    load_order: &T,
    index: usize,
    is_master: bool,
) -> Result<(), Error> {
    match find_first_non_master_position(load_order.plugins()) {
        None if !is_master && index < load_order.plugins().len() => Err(
            Error::NonMasterBeforeMaster,
        ),
        Some(i) if is_master && index > i || !is_master && index < i => Err(
            Error::NonMasterBeforeMaster,
        ),
        _ => Ok(()),
    }
}

fn get_plugin_to_insert_at<T: MutableLoadOrder + ?Sized>(
    load_order: &mut T,
    plugin_name: &str,
    insert_position: usize,
) -> Result<Plugin, Error> {
    if let Some(p) = load_order.index_of(plugin_name) {
        let is_master = load_order.plugins()[p].is_master_file();
        validate_index(load_order, insert_position, is_master)?;

        Ok(load_order.plugins_mut().remove(p))
    } else {
        if !Plugin::is_valid(plugin_name, load_order.game_settings()) {
            return Err(Error::InvalidPlugin(plugin_name.to_string()));
        }

        let plugin = Plugin::new(plugin_name, load_order.game_settings())?;

        validate_index(load_order, insert_position, plugin.is_master_file())?;

        Ok(plugin)
    }
}

fn validate_plugin_names(plugin_names: &[&str], game_settings: &GameSettings) -> Result<(), Error> {
    let unique_plugin_names: HashSet<String> =
        plugin_names.iter().map(|s| s.to_lowercase()).collect();

    if unique_plugin_names.len() != plugin_names.len() {
        return Err(Error::DuplicatePlugin);
    }

    let invalid_plugin = plugin_names.iter().find(
        |p| !Plugin::is_valid(p, game_settings),
    );

    match invalid_plugin {
        Some(x) => Err(Error::InvalidPlugin(x.to_string())),
        None => Ok(()),
    }
}

fn to_plugin(
    plugin_name: &str,
    existing_plugins: &[Plugin],
    game_settings: &GameSettings,
) -> Result<Plugin, Error> {
    match existing_plugins.iter().find(
        |p| p.name_matches(plugin_name),
    ) {
        None => Ok(Plugin::new(plugin_name, game_settings)?),
        Some(x) => Ok(x.clone()),
    }
}

fn map_to_plugins<T: MutableLoadOrder + ?Sized>(
    load_order: &T,
    plugin_names: &[&str],
) -> Result<Vec<Plugin>, Error> {
    plugin_names
        .iter()
        .map(|n| {
            to_plugin(n, load_order.plugins(), load_order.game_settings())
        })
        .collect()
}

fn is_partitioned_by_master_flag(plugins: &[Plugin]) -> bool {
    let plugin_pos = match find_first_non_master_position(plugins) {
        None => return true,
        Some(x) => x,
    };
    match plugins.iter().rposition(|p| p.is_master_file()) {
        None => true,
        Some(master_pos) => master_pos < plugin_pos,
    }
}