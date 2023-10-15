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

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::fs::read_dir;
use std::mem;
use std::path::{Path, PathBuf};

use encoding_rs::WINDOWS_1252;
use rayon::prelude::*;
use unicase::UniCase;

use super::readable::{ReadableLoadOrder, ReadableLoadOrderBase};
use crate::enums::Error;
use crate::game_settings::GameSettings;
use crate::plugin::{has_plugin_extension, trim_dot_ghost, Plugin};
use crate::GameId;

pub trait MutableLoadOrder: ReadableLoadOrder + ReadableLoadOrderBase + Sync {
    fn plugins_mut(&mut self) -> &mut Vec<Plugin>;

    fn insert_position(&self, plugin: &Plugin) -> Option<usize>;

    fn find_plugins(&self) -> Vec<String> {
        // A game might store some plugins outside of its main plugins directory
        // so look for those plugins. They override any of the same names that
        // appear in the main plugins directory, so check for the additional
        // paths first.
        let mut directories = self
            .game_settings()
            .additional_plugins_directories()
            .to_vec();
        directories.push(self.game_settings().plugins_directory());

        find_plugins_in_dirs(&directories, self.game_settings().id())
    }

    fn validate_index(&self, plugin: &Plugin, index: usize) -> Result<(), Error> {
        if plugin.is_master_file() {
            validate_master_file_index(self.plugins(), plugin, index)
        } else {
            validate_non_master_file_index(self.plugins(), plugin, index)
        }
    }

    fn lookup_plugins(&mut self, active_plugin_names: &[&str]) -> Result<Vec<usize>, Error> {
        active_plugin_names
            .par_iter()
            .map(|n| {
                self.plugins()
                    .par_iter()
                    .position_any(|p| p.name_matches(n))
                    .ok_or_else(|| Error::PluginNotFound(n.to_string()))
            })
            .collect()
    }

    fn move_or_insert_plugin_with_index(
        &mut self,
        plugin_name: &str,
        position: usize,
    ) -> Result<usize, Error> {
        if let Some(x) = self.index_of(plugin_name) {
            if x == position {
                return Ok(position);
            }
        }

        let plugin = get_plugin_to_insert_at(self, plugin_name, position)?;

        if position >= self.plugins().len() {
            self.plugins_mut().push(plugin);
            Ok(self.plugins().len() - 1)
        } else {
            self.plugins_mut().insert(position, plugin);
            Ok(position)
        }
    }

    fn deactivate_all(&mut self) {
        for plugin in self.plugins_mut() {
            plugin.deactivate();
        }
    }

    fn replace_plugins(&mut self, plugin_names: &[&str]) -> Result<(), Error> {
        if !are_plugin_names_unique(plugin_names) {
            return Err(Error::DuplicatePlugin);
        }

        let mut plugins = match map_to_plugins(self, plugin_names) {
            Err(x) => return Err(Error::InvalidPlugin(x.to_string())),
            Ok(x) => x,
        };

        validate_load_order(&plugins)?;

        mem::swap(&mut plugins, self.plugins_mut());

        Ok(())
    }

    fn load_unique_plugins(
        &mut self,
        plugin_name_tuples: Vec<(String, bool)>,
        installed_filenames: Vec<String>,
    ) {
        let plugins: Vec<_> = remove_duplicates_icase(plugin_name_tuples, installed_filenames)
            .into_par_iter()
            .filter_map(|(filename, active)| {
                Plugin::with_active(&filename, self.game_settings(), active).ok()
            })
            .collect();

        for plugin in plugins {
            insert(self, plugin);
        }
    }

    fn add_implicitly_active_plugins(&mut self) -> Result<(), Error> {
        let plugin_names = self.game_settings().implicitly_active_plugins().to_vec();

        for plugin_name in plugin_names {
            activate_unvalidated(self, &plugin_name)?;
        }

        Ok(())
    }
}

pub fn load_active_plugins<T, F>(load_order: &mut T, line_mapper: F) -> Result<(), Error>
where
    T: MutableLoadOrder,
    F: Fn(&str) -> Option<String> + Send + Sync,
{
    load_order.deactivate_all();

    let plugin_names = read_plugin_names(
        load_order.game_settings().active_plugins_file(),
        line_mapper,
    )?;

    let plugin_indices: Vec<_> = plugin_names
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
    F: FnMut(&str) -> Option<T> + Send + Sync,
    T: Send,
{
    if !file_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read(file_path)?;

    // This should never fail, as although Windows-1252 has a few unused bytes
    // they get mapped to C1 control characters.
    let decoded_content = WINDOWS_1252
        .decode_without_bom_handling_and_without_replacement(&content)
        .ok_or_else(|| Error::DecodeError("invalid sequence".into()))?;

    Ok(decoded_content.lines().filter_map(line_mapper).collect())
}

pub fn plugin_line_mapper(line: &str) -> Option<String> {
    if line.is_empty() || line.starts_with('#') {
        None
    } else {
        Some(line.to_owned())
    }
}

/// If an ESM has an ESP as a master, the ESP will be loaded directly before the
/// ESM instead of in its usual position. This function "hoists" such ESPs
/// further up the load order.
pub fn hoist_masters(plugins: &mut Vec<Plugin>) -> Result<(), Error> {
    // Store plugins' current positions and where they need to move to.
    // Use a BTreeMap so that if a plugin needs to move for more than one ESM,
    // it will move for the earlier one and so also satisfy the later one, and
    // so that it's possible to iterate over content in order.
    let mut from_to_map: BTreeMap<usize, usize> = BTreeMap::new();

    for (index, plugin) in plugins.iter().enumerate() {
        if !plugin.is_master_file() {
            break;
        }

        for master in plugin.masters()? {
            let pos = plugins
                .iter()
                .position(|p| p.name_matches(&master))
                .unwrap_or(0);
            if pos > index && !plugins[pos].is_master_file() {
                // Need to move the plugin to index, but can't do that while
                // iterating, so store it for later.
                from_to_map.insert(pos, index);
            }
        }
    }

    move_elements(plugins, from_to_map);

    Ok(())
}

pub fn generic_insert_position(plugins: &[Plugin], plugin: &Plugin) -> Option<usize> {
    if plugin.is_master_file() {
        find_first_non_master_position(plugins)
    } else {
        // Check that there isn't a master that would hoist this plugin.
        plugins.iter().filter(|p| p.is_master_file()).position(|p| {
            p.masters()
                .map(|masters| masters.iter().any(|m| plugin.name_matches(m)))
                .unwrap_or(false)
        })
    }
}

fn find_plugins_in_dirs(directories: &[PathBuf], game: GameId) -> Vec<String> {
    let mut dir_entries: Vec<_> = directories
        .iter()
        .flat_map(read_dir)
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|f| f.is_file()).unwrap_or(false))
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|f| has_plugin_extension(f, game))
                .unwrap_or(false)
        })
        .collect();

    // Sort by file modification timestamps, in ascending order. If two timestamps are equal, sort
    // by filenames (in ascending order for Starfield, descending otherwise).
    dir_entries.sort_unstable_by(|e1, e2| {
        let m1 = e1.metadata().and_then(|m| m.modified()).ok();
        let m2 = e2.metadata().and_then(|m| m.modified()).ok();

        match m1.cmp(&m2) {
            Ordering::Equal if game == GameId::Starfield => e1.file_name().cmp(&e2.file_name()),
            Ordering::Equal => e1.file_name().cmp(&e2.file_name()).reverse(),
            x => x,
        }
    });

    let mut set = HashSet::new();

    dir_entries
        .into_iter()
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|filename| set.insert(UniCase::new(trim_dot_ghost(filename).to_string())))
        .collect()
}

fn to_plugin(
    plugin_name: &str,
    existing_plugins: &[Plugin],
    game_settings: &GameSettings,
) -> Result<Plugin, Error> {
    existing_plugins
        .par_iter()
        .find_any(|p| p.name_matches(plugin_name))
        .map_or_else(
            || Plugin::new(plugin_name, game_settings),
            |p| Ok(p.clone()),
        )
}

fn validate_master_file_index(
    plugins: &[Plugin],
    plugin: &Plugin,
    index: usize,
) -> Result<(), Error> {
    let preceding_plugins = if index < plugins.len() {
        &plugins[..index]
    } else {
        plugins
    };

    let previous_master_pos = preceding_plugins
        .iter()
        .rposition(|p| p.is_master_file())
        .unwrap_or(0);

    let masters = plugin.masters()?;
    let master_names: HashSet<_> = masters.iter().map(|m| UniCase::new(m.as_str())).collect();

    // Check that all of the plugins that load between this index and
    // the previous plugin are masters of this plugin.
    if preceding_plugins
        .iter()
        .skip(previous_master_pos + 1)
        .any(|p| !master_names.contains(&UniCase::new(p.name())))
    {
        return Err(Error::NonMasterBeforeMaster);
    }

    // Check that none of the non-masters that load after index are
    // masters of this plugin.
    if let Some(p) = plugins
        .iter()
        .skip(index)
        .filter(|p| !p.is_master_file())
        .find(|p| master_names.contains(&UniCase::new(p.name())))
    {
        Err(Error::UnrepresentedHoist {
            plugin: p.name().to_string(),
            master: plugin.name().to_string(),
        })
    } else {
        Ok(())
    }
}

fn validate_non_master_file_index(
    plugins: &[Plugin],
    plugin: &Plugin,
    index: usize,
) -> Result<(), Error> {
    // Check that there aren't any earlier master files that have this
    // plugin as a master.
    for master_file in plugins.iter().take(index).filter(|p| p.is_master_file()) {
        if master_file
            .masters()?
            .iter()
            .any(|m| plugin.name_matches(m))
        {
            return Err(Error::UnrepresentedHoist {
                plugin: plugin.name().to_string(),
                master: master_file.name().to_string(),
            });
        }
    }

    // Check that the next master file has this plugin as a master.
    let next_master = match plugins.iter().skip(index).find(|p| p.is_master_file()) {
        None => return Ok(()),
        Some(p) => p,
    };

    if next_master
        .masters()?
        .iter()
        .any(|m| plugin.name_matches(m))
    {
        Ok(())
    } else {
        Err(Error::NonMasterBeforeMaster)
    }
}

fn map_to_plugins<T: ReadableLoadOrderBase + Sync + ?Sized>(
    load_order: &T,
    plugin_names: &[&str],
) -> Result<Vec<Plugin>, Error> {
    plugin_names
        .par_iter()
        .map(|n| to_plugin(n, load_order.plugins(), load_order.game_settings_base()))
        .collect()
}

fn insert<T: MutableLoadOrder + ?Sized>(load_order: &mut T, plugin: Plugin) -> usize {
    match load_order.insert_position(&plugin) {
        Some(position) => {
            load_order.plugins_mut().insert(position, plugin);
            position
        }
        None => {
            load_order.plugins_mut().push(plugin);
            load_order.plugins().len() - 1
        }
    }
}

fn move_elements<T>(vec: &mut Vec<T>, mut from_to_indices: BTreeMap<usize, usize>) {
    // Move elements around. Moving elements doesn't change from_index values,
    // as we're iterating from earliest index to latest, but to_index values can
    // become incorrect, e.g. (5, 2), (6, 3), (7, 1) will insert an element
    // before index 3 so that should become 4, but 1 is still correct.
    // Keeping track of what indices need offsets is probably not worth it as
    // this function is likely to be called with empty or very small maps, so
    // just loop through it after each move and increment any affected to_index
    // values.
    while let Some((from_index, to_index)) = from_to_indices.pop_first() {
        let element = vec.remove(from_index);
        vec.insert(to_index, element);

        for value in from_to_indices.values_mut() {
            if *value > to_index {
                *value += 1;
            }
        }
    }
}

fn get_plugin_to_insert_at<T: MutableLoadOrder + ?Sized>(
    load_order: &mut T,
    plugin_name: &str,
    insert_position: usize,
) -> Result<Plugin, Error> {
    if let Some(p) = load_order.index_of(plugin_name) {
        let plugin = &load_order.plugins()[p];
        load_order.validate_index(plugin, insert_position)?;

        Ok(load_order.plugins_mut().remove(p))
    } else {
        let plugin = Plugin::new(plugin_name, load_order.game_settings())
            .map_err(|_| Error::InvalidPlugin(plugin_name.to_string()))?;

        load_order.validate_index(&plugin, insert_position)?;

        Ok(plugin)
    }
}

fn are_plugin_names_unique(plugin_names: &[&str]) -> bool {
    let unique_plugin_names: HashSet<_> =
        plugin_names.par_iter().map(|s| UniCase::new(s)).collect();

    unique_plugin_names.len() == plugin_names.len()
}

fn validate_load_order(plugins: &[Plugin]) -> Result<(), Error> {
    let first_non_master_pos = match find_first_non_master_position(plugins) {
        None => return Ok(()),
        Some(x) => x,
    };

    let last_master_pos = match plugins.iter().rposition(|p| p.is_master_file()) {
        None => return Ok(()),
        Some(x) => x,
    };

    let mut plugin_names: HashSet<_> = HashSet::new();

    // Add each plugin that isn't a master file to the hashset.
    // When a master file is encountered, remove its masters from the hashset.
    // If there are any plugins left in the hashset, they weren't hoisted there,
    // so fail the check.
    if first_non_master_pos < last_master_pos {
        for plugin in plugins
            .iter()
            .skip(first_non_master_pos)
            .take(last_master_pos - first_non_master_pos + 1)
        {
            if !plugin.is_master_file() {
                plugin_names.insert(UniCase::new(plugin.name().to_string()));
            } else {
                for master in plugin.masters()? {
                    plugin_names.remove(&UniCase::new(master.clone()));
                }

                if !plugin_names.is_empty() {
                    return Err(Error::NonMasterBeforeMaster);
                }
            }
        }
    }

    // Now check in reverse that no master file depends on a non-master that
    // loads after it.
    plugin_names.clear();
    for plugin in plugins.iter().rev() {
        if !plugin.is_master_file() {
            plugin_names.insert(UniCase::new(plugin.name().to_string()));
        } else if let Some(m) = plugin
            .masters()?
            .iter()
            .find(|m| plugin_names.contains(&UniCase::new(m.to_string())))
        {
            return Err(Error::UnrepresentedHoist {
                plugin: m.clone(),
                master: plugin.name().to_string(),
            });
        }
    }

    Ok(())
}

fn remove_duplicates_icase(
    plugin_tuples: Vec<(String, bool)>,
    filenames: Vec<String>,
) -> Vec<(String, bool)> {
    let mut set: HashSet<_> = HashSet::with_capacity(filenames.len());

    let mut unique_tuples: Vec<(String, bool)> = plugin_tuples
        .into_iter()
        .rev()
        .filter(|(string, _)| set.insert(UniCase::new(trim_dot_ghost(string).to_string())))
        .collect();

    unique_tuples.reverse();

    let unique_file_tuples_iter = filenames
        .into_iter()
        .filter(|string| set.insert(UniCase::new(trim_dot_ghost(string).to_string())))
        .map(|f| (f, false));

    unique_tuples.extend(unique_file_tuples_iter);

    unique_tuples
}

fn activate_unvalidated<T: MutableLoadOrder + ?Sized>(
    load_order: &mut T,
    filename: &str,
) -> Result<(), Error> {
    if let Some(plugin) = load_order
        .plugins_mut()
        .iter_mut()
        .find(|p| p.name_matches(filename))
    {
        plugin.activate()
    } else {
        // Ignore any errors trying to load the plugin to save checking if it's
        // valid and then loading it if it is.
        Plugin::with_active(filename, load_order.game_settings(), true)
            .map(|plugin| {
                insert(load_order, plugin);
            })
            .or(Ok(()))
    }
}

fn find_first_non_master_position(plugins: &[Plugin]) -> Option<usize> {
    plugins.iter().position(|p| !p.is_master_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::enums::GameId;
    use crate::game_settings::GameSettings;
    use crate::load_order::tests::*;
    use crate::tests::copy_to_test_dir;

    use tempfile::tempdir;

    struct TestLoadOrder {
        game_settings: GameSettings,
        plugins: Vec<Plugin>,
    }

    impl ReadableLoadOrderBase for TestLoadOrder {
        fn game_settings_base(&self) -> &GameSettings {
            &self.game_settings
        }

        fn plugins(&self) -> &[Plugin] {
            &self.plugins
        }
    }

    impl MutableLoadOrder for TestLoadOrder {
        fn plugins_mut(&mut self) -> &mut Vec<Plugin> {
            &mut self.plugins
        }

        fn insert_position(&self, plugin: &Plugin) -> Option<usize> {
            generic_insert_position(self.plugins(), plugin)
        }
    }

    fn prepare(game_path: &Path) -> GameSettings {
        let settings = game_settings_for_test(GameId::SkyrimSE, game_path);

        copy_to_test_dir("Blank.esm", settings.master_file(), &settings);
        copy_to_test_dir("Blank.esm", "Blank.esm", &settings);
        copy_to_test_dir("Blank.esp", "Blank.esp", &settings);
        copy_to_test_dir("Blank - Different.esp", "Blank - Different.esp", &settings);
        copy_to_test_dir(
            "Blank - Plugin Dependent.esp",
            "Blank - Plugin Dependent.esm",
            &settings,
        );

        settings
    }

    fn prepare_load_order(game_dir: &Path) -> TestLoadOrder {
        let (game_settings, plugins) = mock_game_files(GameId::Oblivion, game_dir);
        TestLoadOrder {
            game_settings,
            plugins,
        }
    }

    fn prepare_hoisted_load_order(game_path: &Path) -> TestLoadOrder {
        let load_order = prepare_load_order(game_path);

        let plugins_dir = &load_order.game_settings().plugins_directory();
        copy_to_test_dir(
            "Blank - Different.esm",
            "Blank - Different.esm",
            load_order.game_settings(),
        );
        set_master_flag(&plugins_dir.join("Blank - Different.esm"), false).unwrap();
        copy_to_test_dir(
            "Blank - Different Master Dependent.esm",
            "Blank - Different Master Dependent.esm",
            load_order.game_settings(),
        );

        load_order
    }

    fn prepare_plugins(game_path: &Path, blank_esp_source: &str) -> Vec<Plugin> {
        let settings = game_settings_for_test(GameId::SkyrimSE, game_path);

        copy_to_test_dir("Blank.esm", settings.master_file(), &settings);
        copy_to_test_dir(blank_esp_source, "Blank.esp", &settings);

        vec![
            Plugin::new(settings.master_file(), &settings).unwrap(),
            Plugin::new("Blank.esp", &settings).unwrap(),
        ]
    }

    #[test]
    fn validate_index_should_succeed_for_a_master_plugin_and_index_directly_after_a_master() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare_load_order(&tmp_dir.path());

        let plugin = Plugin::new("Blank.esm", load_order.game_settings()).unwrap();
        assert!(load_order.validate_index(&plugin, 1).is_ok());
    }

    #[test]
    fn validate_index_should_succeed_for_a_master_plugin_and_index_after_a_hoisted_non_master() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare_hoisted_load_order(&tmp_dir.path());

        let plugin = Plugin::new("Blank - Different.esm", load_order.game_settings()).unwrap();
        load_order.plugins.insert(1, plugin);

        let plugin = Plugin::new(
            "Blank - Different Master Dependent.esm",
            load_order.game_settings(),
        )
        .unwrap();
        assert!(load_order.validate_index(&plugin, 2).is_ok());
    }

    #[test]
    fn validate_index_should_error_for_a_master_plugin_and_index_after_unrelated_non_masters() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare_hoisted_load_order(&tmp_dir.path());

        let plugin = Plugin::new("Blank - Different.esm", load_order.game_settings()).unwrap();
        load_order.plugins.insert(1, plugin);

        let plugin = Plugin::new("Blank.esm", load_order.game_settings()).unwrap();
        assert!(load_order.validate_index(&plugin, 4).is_err());
    }

    #[test]
    fn validate_index_should_error_for_a_master_plugin_that_has_a_later_non_master_as_a_master() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare_hoisted_load_order(&tmp_dir.path());

        let plugin = Plugin::new("Blank - Different.esm", load_order.game_settings()).unwrap();
        load_order.plugins.insert(2, plugin);

        let plugin = Plugin::new(
            "Blank - Different Master Dependent.esm",
            load_order.game_settings(),
        )
        .unwrap();
        assert!(load_order.validate_index(&plugin, 1).is_err());
    }

    #[test]
    fn validate_index_should_succeed_for_a_non_master_plugin_and_an_index_with_no_later_masters() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare_load_order(&tmp_dir.path());

        let plugin =
            Plugin::new("Blank - Master Dependent.esp", load_order.game_settings()).unwrap();
        assert!(load_order.validate_index(&plugin, 2).is_ok());
    }

    #[test]
    fn validate_index_should_succeed_for_a_non_master_plugin_that_is_a_master_of_the_next_master_file(
    ) {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare_hoisted_load_order(&tmp_dir.path());

        let plugin = Plugin::new(
            "Blank - Different Master Dependent.esm",
            load_order.game_settings(),
        )
        .unwrap();
        load_order.plugins.insert(1, plugin);

        let plugin = Plugin::new("Blank - Different.esm", load_order.game_settings()).unwrap();
        assert!(load_order.validate_index(&plugin, 1).is_ok());
    }

    #[test]
    fn validate_index_should_error_for_a_non_master_plugin_that_is_not_a_master_of_the_next_master_file(
    ) {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare_load_order(&tmp_dir.path());

        let plugin =
            Plugin::new("Blank - Master Dependent.esp", load_order.game_settings()).unwrap();
        assert!(load_order.validate_index(&plugin, 0).is_err());
    }

    #[test]
    fn validate_index_should_error_for_a_non_master_plugin_and_an_index_not_before_a_master_that_depends_on_it(
    ) {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare_hoisted_load_order(&tmp_dir.path());

        let plugin = Plugin::new(
            "Blank - Different Master Dependent.esm",
            load_order.game_settings(),
        )
        .unwrap();
        load_order.plugins.insert(1, plugin);

        let plugin = Plugin::new("Blank - Different.esm", load_order.game_settings()).unwrap();
        assert!(load_order.validate_index(&plugin, 2).is_err());
    }

    #[test]
    fn find_plugins_in_dirs_should_sort_files_by_modification_timestamp() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare_load_order(&tmp_dir.path());

        let result = find_plugins_in_dirs(
            &[load_order.game_settings.plugins_directory()],
            load_order.game_settings.id(),
        );

        let plugin_names = [
            load_order.game_settings.master_file(),
            "Blank.esm",
            "Blank.esp",
            "Blank - Different.esp",
            "Blank - Master Dependent.esp",
            "Blàñk.esp",
        ];

        assert_eq!(plugin_names.as_slice(), result);
    }

    #[test]
    fn find_plugins_in_dirs_should_sort_files_by_descending_filename_if_timestamps_are_equal() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare_load_order(&tmp_dir.path());

        let timestamp = 1321010051;
        filetime::set_file_mtime(
            load_order
                .game_settings
                .plugins_directory()
                .join("Blank - Different.esp"),
            filetime::FileTime::from_unix_time(timestamp, 0),
        )
        .unwrap();
        filetime::set_file_mtime(
            load_order
                .game_settings
                .plugins_directory()
                .join("Blank - Master Dependent.esp"),
            filetime::FileTime::from_unix_time(timestamp, 0),
        )
        .unwrap();

        let result = find_plugins_in_dirs(
            &[load_order.game_settings.plugins_directory()],
            load_order.game_settings.id(),
        );

        let plugin_names = [
            load_order.game_settings.master_file(),
            "Blank.esm",
            "Blank.esp",
            "Blank - Master Dependent.esp",
            "Blank - Different.esp",
            "Blàñk.esp",
        ];

        assert_eq!(plugin_names.as_slice(), result);
    }

    #[test]
    fn find_plugins_in_dirs_should_sort_files_by_ascending_filename_if_timestamps_are_equal_and_game_is_starfield(
    ) {
        let tmp_dir = tempdir().unwrap();
        let (game_settings, plugins) = mock_game_files(GameId::Starfield, &tmp_dir.path());
        let load_order = TestLoadOrder {
            game_settings,
            plugins,
        };

        let timestamp = 1321009991;
        filetime::set_file_mtime(
            load_order
                .game_settings
                .plugins_directory()
                .join("Blank - Different.esp"),
            filetime::FileTime::from_unix_time(timestamp, 0),
        )
        .unwrap();
        filetime::set_file_mtime(
            load_order
                .game_settings
                .plugins_directory()
                .join("Blank.esp"),
            filetime::FileTime::from_unix_time(timestamp, 0),
        )
        .unwrap();

        let result = find_plugins_in_dirs(
            &[load_order.game_settings.plugins_directory()],
            load_order.game_settings.id(),
        );

        let plugin_names = [
            load_order.game_settings.master_file(),
            "Blank.esm",
            "Blank - Different.esp",
            "Blank.esp",
            "Blank - Master Dependent.esp",
            "Blàñk.esp",
        ];

        assert_eq!(plugin_names.as_slice(), result);
    }

    #[test]
    fn move_elements_should_correct_later_indices_to_account_for_earlier_moves() {
        let mut vec = vec![0, 1, 2, 3, 4, 5, 6, 7, 8];
        let mut from_to_indices = BTreeMap::new();
        from_to_indices.insert(6, 3);
        from_to_indices.insert(5, 2);
        from_to_indices.insert(7, 1);

        move_elements(&mut vec, from_to_indices);

        assert_eq!(vec![0, 7, 1, 5, 2, 6, 3, 4, 8], vec);
    }

    #[test]
    fn validate_load_order_should_be_ok_if_there_are_only_master_files() {
        let tmp_dir = tempdir().unwrap();
        let settings = prepare(&tmp_dir.path());

        let plugins = vec![
            Plugin::new(settings.master_file(), &settings).unwrap(),
            Plugin::new("Blank.esm", &settings).unwrap(),
        ];

        assert!(validate_load_order(&plugins).is_ok());
    }

    #[test]
    fn validate_load_order_should_be_ok_if_there_are_no_master_files() {
        let tmp_dir = tempdir().unwrap();
        let settings = prepare(&tmp_dir.path());

        let plugins = vec![
            Plugin::new("Blank.esp", &settings).unwrap(),
            Plugin::new("Blank - Different.esp", &settings).unwrap(),
        ];

        assert!(validate_load_order(&plugins).is_ok());
    }

    #[test]
    fn validate_load_order_should_be_ok_if_master_files_are_before_all_others() {
        let tmp_dir = tempdir().unwrap();
        let settings = prepare(&tmp_dir.path());

        let plugins = vec![
            Plugin::new("Blank.esm", &settings).unwrap(),
            Plugin::new("Blank.esp", &settings).unwrap(),
        ];

        assert!(validate_load_order(&plugins).is_ok());
    }

    #[test]
    fn validate_load_order_should_be_ok_if_hoisted_non_masters_load_before_masters() {
        let tmp_dir = tempdir().unwrap();
        let settings = prepare(&tmp_dir.path());

        let plugins = vec![
            Plugin::new("Blank.esm", &settings).unwrap(),
            Plugin::new("Blank.esp", &settings).unwrap(),
            Plugin::new("Blank - Plugin Dependent.esm", &settings).unwrap(),
        ];

        assert!(validate_load_order(&plugins).is_ok());
    }

    #[test]
    fn validate_load_order_should_error_if_non_masters_are_hoisted_earlier_than_needed() {
        let tmp_dir = tempdir().unwrap();
        let settings = prepare(&tmp_dir.path());

        let plugins = vec![
            Plugin::new("Blank.esp", &settings).unwrap(),
            Plugin::new("Blank.esm", &settings).unwrap(),
            Plugin::new("Blank - Plugin Dependent.esm", &settings).unwrap(),
        ];

        assert!(validate_load_order(&plugins).is_err());
    }

    #[test]
    fn validate_load_order_should_error_if_master_files_load_before_non_masters_they_have_as_masters(
    ) {
        let tmp_dir = tempdir().unwrap();
        let settings = prepare(&tmp_dir.path());

        let plugins = vec![
            Plugin::new("Blank.esm", &settings).unwrap(),
            Plugin::new("Blank - Plugin Dependent.esm", &settings).unwrap(),
            Plugin::new("Blank.esp", &settings).unwrap(),
        ];

        assert!(validate_load_order(&plugins).is_err());
    }

    #[test]
    fn find_first_non_master_should_find_a_normal_esp() {
        let tmp_dir = tempdir().unwrap();
        let plugins = prepare_plugins(&tmp_dir.path(), "Blank.esp");

        let first_non_master = super::find_first_non_master_position(&plugins);
        assert_eq!(1, first_non_master.unwrap());
    }

    #[test]
    fn find_first_non_master_should_find_a_light_flagged_esp() {
        let tmp_dir = tempdir().unwrap();
        let plugins = prepare_plugins(&tmp_dir.path(), "Blank.esl");

        let first_non_master = super::find_first_non_master_position(&plugins);
        assert_eq!(1, first_non_master.unwrap());
    }
}
