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
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use unicase::eq;

use super::mutable::{
    generic_insert_position, hoist_masters, load_active_plugins, plugin_line_mapper,
    read_plugin_names, MutableLoadOrder,
};
use super::readable::{ReadableLoadOrder, ReadableLoadOrderBase};
use super::strict_encode;
use super::writable::{
    activate, add, create_parent_dirs, deactivate, remove, set_active_plugins, WritableLoadOrder,
};
use crate::enums::Error;
use crate::game_settings::GameSettings;
use crate::plugin::{trim_dot_ghost, Plugin};

#[derive(Clone, Debug)]
pub struct TextfileBasedLoadOrder {
    game_settings: GameSettings,
    plugins: Vec<Plugin>,
}

impl TextfileBasedLoadOrder {
    pub fn new(game_settings: GameSettings) -> Self {
        Self {
            game_settings,
            plugins: Vec::new(),
        }
    }

    fn read_from_load_order_file(&self) -> Result<Vec<(String, bool)>, Error> {
        match self.game_settings().load_order_file() {
            Some(file_path) => read_utf8_plugin_names(file_path, load_order_line_mapper)
                .or_else(|_| read_plugin_names(file_path, load_order_line_mapper)),
            None => Ok(Vec::new()),
        }
    }

    fn read_from_active_plugins_file(&self) -> Result<Vec<(String, bool)>, Error> {
        read_plugin_names(
            self.game_settings().active_plugins_file(),
            active_plugin_line_mapper,
        )
    }

    fn save_load_order(&self) -> Result<(), Error> {
        if let Some(file_path) = self.game_settings().load_order_file() {
            create_parent_dirs(file_path)?;

            let file = File::create(file_path)?;
            let mut writer = BufWriter::new(file);
            for plugin_name in self.plugin_names() {
                writeln!(writer, "{}", plugin_name)?;
            }
        }
        Ok(())
    }

    fn save_active_plugins(&self) -> Result<(), Error> {
        create_parent_dirs(self.game_settings().active_plugins_file())?;

        let file = File::create(self.game_settings().active_plugins_file())?;
        let mut writer = BufWriter::new(file);
        for plugin_name in self.active_plugin_names() {
            writer.write_all(&strict_encode(plugin_name)?)?;
            writeln!(writer)?;
        }

        Ok(())
    }
}

impl ReadableLoadOrderBase for TextfileBasedLoadOrder {
    fn game_settings_base(&self) -> &GameSettings {
        &self.game_settings
    }

    fn plugins(&self) -> &[Plugin] {
        &self.plugins
    }
}

impl MutableLoadOrder for TextfileBasedLoadOrder {
    fn plugins_mut(&mut self) -> &mut Vec<Plugin> {
        &mut self.plugins
    }

    fn insert_position(&self, plugin: &Plugin) -> Option<usize> {
        let is_game_master = eq(plugin.name(), self.game_settings().master_file());

        if is_game_master {
            if self.plugins().is_empty() {
                None
            } else {
                Some(0)
            }
        } else {
            generic_insert_position(self.plugins(), plugin)
        }
    }
}

impl WritableLoadOrder for TextfileBasedLoadOrder {
    fn game_settings_mut(&mut self) -> &mut GameSettings {
        &mut self.game_settings
    }

    fn load(&mut self) -> Result<(), Error> {
        self.plugins_mut().clear();

        let load_order_file_exists = self
            .game_settings()
            .load_order_file()
            .map(|p| p.exists())
            .unwrap_or(false);

        let plugin_tuples = if load_order_file_exists {
            self.read_from_load_order_file()?
        } else {
            self.read_from_active_plugins_file()?
        };

        let filenames = self.find_plugins_sorted();
        self.load_unique_plugins(plugin_tuples, filenames);

        if load_order_file_exists {
            load_active_plugins(self, plugin_line_mapper)?;
        }

        hoist_masters(&mut self.plugins)?;

        self.add_implicitly_active_plugins()?;

        Ok(())
    }

    fn save(&mut self) -> Result<(), Error> {
        self.save_load_order()?;
        self.save_active_plugins()
    }

    fn add(&mut self, plugin_name: &str) -> Result<usize, Error> {
        add(self, plugin_name)
    }

    fn remove(&mut self, plugin_name: &str) -> Result<(), Error> {
        remove(self, plugin_name)
    }

    fn set_load_order(&mut self, plugin_names: &[&str]) -> Result<(), Error> {
        if plugin_names.is_empty() || !eq(plugin_names[0], self.game_settings().master_file()) {
            return Err(Error::GameMasterMustLoadFirst);
        }

        self.replace_plugins(plugin_names)
    }

    fn set_plugin_index(&mut self, plugin_name: &str, position: usize) -> Result<usize, Error> {
        if position != 0
            && !self.plugins().is_empty()
            && eq(plugin_name, self.game_settings().master_file())
        {
            return Err(Error::GameMasterMustLoadFirst);
        }
        if position == 0 && !eq(plugin_name, self.game_settings().master_file()) {
            return Err(Error::GameMasterMustLoadFirst);
        }

        self.move_or_insert_plugin_with_index(plugin_name, position)
    }

    fn is_self_consistent(&self) -> Result<bool, Error> {
        match check_self_consistency(self.game_settings())? {
            SelfConsistency::Inconsistent => Ok(false),
            _ => Ok(true),
        }
    }

    /// A textfile-based load order is ambiguous when it's not self-consistent
    /// (because an app that prefers loadorder.txt may give a different load
    /// order to one that prefers plugins.txt) or when there are installed
    /// plugins that are not present in one or both of the text files.
    fn is_ambiguous(&self) -> Result<bool, Error> {
        let plugin_names = match check_self_consistency(self.game_settings())? {
            SelfConsistency::Inconsistent => {
                return Ok(true);
            }
            SelfConsistency::ConsistentWithNames(plugin_names) => plugin_names,
            SelfConsistency::ConsistentNoLoadOrderFile => read_plugin_names(
                self.game_settings().active_plugins_file(),
                plugin_line_mapper,
            )?,
            SelfConsistency::ConsistentOnlyLoadOrderFile(load_order_file) => {
                read_utf8_plugin_names(&load_order_file, plugin_line_mapper)
                    .or_else(|_| read_plugin_names(&load_order_file, plugin_line_mapper))?
            }
        };

        let set: HashSet<String> = plugin_names
            .into_iter()
            .map(|name| trim_dot_ghost(&name).to_lowercase())
            .collect();

        let all_plugins_listed = self
            .plugins
            .iter()
            .all(|plugin| set.contains(&plugin.name().to_lowercase()));

        Ok(!all_plugins_listed)
    }

    fn activate(&mut self, plugin_name: &str) -> Result<(), Error> {
        activate(self, plugin_name)
    }

    fn deactivate(&mut self, plugin_name: &str) -> Result<(), Error> {
        deactivate(self, plugin_name)
    }

    fn set_active_plugins(&mut self, active_plugin_names: &[&str]) -> Result<(), Error> {
        set_active_plugins(self, active_plugin_names)
    }
}

pub fn read_utf8_plugin_names<F, T>(file_path: &Path, line_mapper: F) -> Result<Vec<T>, Error>
where
    F: Fn(&str) -> Option<T> + Send + Sync,
    T: Send,
{
    if !file_path.exists() {
        return Ok(Vec::new());
    }

    let mut content: String = String::new();
    let mut file = File::open(file_path)?;
    file.read_to_string(&mut content)?;

    Ok(content.lines().filter_map(line_mapper).collect())
}

enum SelfConsistency {
    ConsistentNoLoadOrderFile,
    ConsistentOnlyLoadOrderFile(PathBuf),
    ConsistentWithNames(Vec<String>),
    Inconsistent,
}

fn check_self_consistency(game_settings: &GameSettings) -> Result<SelfConsistency, Error> {
    match game_settings.load_order_file() {
        None => Ok(SelfConsistency::ConsistentNoLoadOrderFile),
        Some(load_order_file) => {
            if !load_order_file.exists() {
                return Ok(SelfConsistency::ConsistentNoLoadOrderFile);
            }

            if !game_settings.active_plugins_file().exists() {
                return Ok(SelfConsistency::ConsistentOnlyLoadOrderFile(
                    load_order_file.clone(),
                ));
            }

            // First get load order according to loadorder.txt.
            let load_order_plugin_names =
                read_utf8_plugin_names(load_order_file, plugin_line_mapper)
                    .or_else(|_| read_plugin_names(load_order_file, plugin_line_mapper))?;

            // Get load order from plugins.txt.
            let active_plugin_names =
                read_plugin_names(game_settings.active_plugins_file(), plugin_line_mapper)?;

            let are_equal = load_order_plugin_names
                .iter()
                .filter(|l| active_plugin_names.iter().any(|a| plugin_names_match(a, l)))
                .zip(active_plugin_names.iter())
                .all(|(l, a)| plugin_names_match(l, a));

            if are_equal {
                Ok(SelfConsistency::ConsistentWithNames(
                    load_order_plugin_names,
                ))
            } else {
                Ok(SelfConsistency::Inconsistent)
            }
        }
    }
}

fn load_order_line_mapper(line: &str) -> Option<(String, bool)> {
    plugin_line_mapper(line).map(|s| (s, false))
}

fn active_plugin_line_mapper(line: &str) -> Option<(String, bool)> {
    plugin_line_mapper(line).map(|s| (s, true))
}

fn plugin_names_match(name1: &str, name2: &str) -> bool {
    eq(trim_dot_ghost(name1), trim_dot_ghost(name2))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::enums::GameId;
    use crate::load_order::tests::*;
    use crate::tests::copy_to_test_dir;
    use filetime::{set_file_times, FileTime};
    use std::fs::{remove_dir_all, File};
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;

    fn prepare(game_id: GameId, game_dir: &Path) -> TextfileBasedLoadOrder {
        let (game_settings, plugins) = mock_game_files(game_id, game_dir);
        TextfileBasedLoadOrder {
            game_settings,
            plugins,
        }
    }

    fn write_file(path: &Path) {
        let mut file = File::create(&path).unwrap();
        writeln!(file).unwrap();
    }

    #[test]
    fn insert_position_should_return_zero_if_given_the_game_master_plugin() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let plugin = Plugin::new("Skyrim.esm", &load_order.game_settings()).unwrap();
        let position = load_order.insert_position(&plugin);

        assert_eq!(0, position.unwrap());
    }

    #[test]
    fn insert_position_should_return_none_for_the_game_master_if_no_plugins_are_loaded() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        load_order.plugins_mut().clear();

        let plugin = Plugin::new("Skyrim.esm", &load_order.game_settings()).unwrap();
        let position = load_order.insert_position(&plugin);

        assert!(position.is_none());
    }

    #[test]
    fn insert_position_should_return_none_if_given_a_non_master_plugin() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let plugin =
            Plugin::new("Blank - Master Dependent.esp", &load_order.game_settings()).unwrap();
        let position = load_order.insert_position(&plugin);

        assert_eq!(None, position);
    }

    #[test]
    fn insert_position_should_return_the_first_non_master_plugin_index_if_given_a_master_plugin() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let plugin = Plugin::new("Blank.esm", &load_order.game_settings()).unwrap();
        let position = load_order.insert_position(&plugin);

        assert_eq!(1, position.unwrap());
    }

    #[test]
    fn insert_position_should_return_none_if_no_non_masters_are_present() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        // Remove non-master plugins from the load order.
        load_order.plugins_mut().retain(|p| p.is_master_file());

        let plugin = Plugin::new("Blank.esm", &load_order.game_settings()).unwrap();
        let position = load_order.insert_position(&plugin);

        assert_eq!(None, position);
    }

    #[test]
    fn load_should_reload_existing_plugins() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        assert!(!load_order.plugins()[1].is_master_file());
        copy_to_test_dir("Blank.esm", "Blank.esp", &load_order.game_settings());
        let plugin_path = load_order
            .game_settings()
            .plugins_directory()
            .join("Blank.esp");
        set_file_times(&plugin_path, FileTime::zero(), FileTime::zero()).unwrap();

        load_order.load().unwrap();

        assert!(load_order.plugins()[1].is_master_file());
    }

    #[test]
    fn load_should_remove_plugins_that_fail_to_load() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        assert!(load_order.index_of("Blank.esp").is_some());
        assert!(load_order.index_of("Blank - Different.esp").is_some());

        let plugin_path = load_order
            .game_settings()
            .plugins_directory()
            .join("Blank.esp");
        write_file(&plugin_path);
        set_file_times(&plugin_path, FileTime::zero(), FileTime::zero()).unwrap();

        let plugin_path = load_order
            .game_settings()
            .plugins_directory()
            .join("Blank - Different.esp");
        write_file(&plugin_path);
        set_file_times(&plugin_path, FileTime::zero(), FileTime::zero()).unwrap();

        load_order.load().unwrap();
        assert!(load_order.index_of("Blank.esp").is_none());
        assert!(load_order.index_of("Blank - Different.esp").is_none());
    }

    #[test]
    fn load_should_get_load_order_from_load_order_file() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let expected_filenames = vec![
            "Skyrim.esm",
            "Blank.esm",
            "Blàñk.esp",
            "Blank - Master Dependent.esp",
            "Blank - Different.esp",
            "Blank.esp",
            "missing.esp",
        ];
        write_load_order_file(load_order.game_settings(), &expected_filenames);

        load_order.load().unwrap();
        assert_eq!(
            &expected_filenames[..6],
            load_order.plugin_names().as_slice()
        );
    }

    #[test]
    fn load_should_hoist_non_masters_that_masters_depend_on_to_load_before_their_dependents() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

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

        let expected_filenames = vec![
            "Blank - Master Dependent.esp",
            "Blank.esm",
            "Blank - Different Master Dependent.esm",
            "Blank - Different.esp",
            "Blàñk.esp",
            "Blank.esp",
            "Skyrim.esm",
        ];
        write_load_order_file(load_order.game_settings(), &expected_filenames);

        load_order.load().unwrap();

        let expected_filenames = vec![
            "Skyrim.esm",
            "Blank.esm",
            "Blank - Different.esm",
            "Blank - Different Master Dependent.esm",
            "Blank - Master Dependent.esp",
            "Blank - Different.esp",
            "Blàñk.esp",
            "Blank.esp",
        ];

        assert_eq!(expected_filenames, load_order.plugin_names());
    }

    #[test]
    fn load_should_read_load_order_file_as_windows_1252_if_not_utf8() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let expected_filenames = vec![
            "Skyrim.esm",
            "Blank.esm",
            "Blàñk.esp",
            "Blank - Master Dependent.esp",
            "Blank - Different.esp",
            "Blank.esp",
            "missing.esp",
        ];

        let mut file =
            File::create(&load_order.game_settings().load_order_file().unwrap()).unwrap();

        for filename in &expected_filenames {
            file.write_all(&strict_encode(filename).unwrap()).unwrap();
            writeln!(file).unwrap();
        }

        load_order.load().unwrap();
        assert_eq!(
            &expected_filenames[..6],
            load_order.plugin_names().as_slice()
        );
    }

    #[test]
    fn load_should_get_load_order_from_active_plugins_file_if_load_order_file_does_not_exist() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        write_active_plugins_file(
            load_order.game_settings(),
            &["Blank.esp", "Blank - Master Dependent.esp"],
        );

        load_order.load().unwrap();

        let expected_filenames = vec![
            load_order.game_settings().master_file(),
            "Blank.esm",
            "Blank.esp",
            "Blank - Master Dependent.esp",
            "Blank - Different.esp",
            "Blàñk.esp",
        ];

        assert_eq!(expected_filenames, load_order.plugin_names());
    }

    #[test]
    fn load_should_add_missing_plugins() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        assert!(load_order.index_of("Blank.esm").is_none());
        assert!(load_order
            .index_of("Blank - Master Dependent.esp")
            .is_none());
        assert!(load_order.index_of("Blàñk.esp").is_none());

        load_order.load().unwrap();

        assert!(load_order.index_of("Blank.esm").is_some());
        assert!(load_order
            .index_of("Blank - Master Dependent.esp")
            .is_some());
        assert!(load_order.index_of("Blàñk.esp").is_some());
    }

    #[test]
    fn load_should_add_missing_implicitly_active_plugins_after_other_missing_masters() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        copy_to_test_dir("Blank.esm", "Update.esm", &load_order.game_settings());
        load_order.load().unwrap();
        assert_eq!(Some(2), load_order.index_of("Update.esm"));
        assert!(load_order.is_active("Update.esm"));
    }

    #[test]
    fn load_should_empty_the_load_order_if_the_plugins_directory_does_not_exist() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());
        tmp_dir.close().unwrap();

        load_order.load().unwrap();

        assert!(load_order.plugins().is_empty());
    }

    #[test]
    fn load_should_load_plugin_states_from_active_plugins_file() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        write_active_plugins_file(
            load_order.game_settings(),
            &["Blank.esm", "Blank - Master Dependent.esp"],
        );

        load_order.load().unwrap();
        let expected_filenames = vec!["Skyrim.esm", "Blank.esm", "Blank - Master Dependent.esp"];

        assert_eq!(expected_filenames, load_order.active_plugin_names());
    }

    #[test]
    fn load_should_decode_active_plugins_file_from_windows_1252() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        write_active_plugins_file(load_order.game_settings(), &["Blàñk.esp", "Blank.esm"]);

        load_order.load().unwrap();
        let expected_filenames = vec!["Skyrim.esm", "Blank.esm", "Blàñk.esp"];

        assert_eq!(expected_filenames, load_order.active_plugin_names());
    }

    #[test]
    fn load_should_handle_crlf_and_lf_in_active_plugins_file() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        write_active_plugins_file(load_order.game_settings(), &["Blàñk.esp", "Blank.esm\r"]);

        load_order.load().unwrap();
        let expected_filenames = vec!["Skyrim.esm", "Blank.esm", "Blàñk.esp"];

        assert_eq!(expected_filenames, load_order.active_plugin_names());
    }

    #[test]
    fn load_should_ignore_active_plugins_file_lines_starting_with_a_hash() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        write_active_plugins_file(
            load_order.game_settings(),
            &["#Blank.esp", "Blàñk.esp", "Blank.esm"],
        );

        load_order.load().unwrap();
        let expected_filenames = vec!["Skyrim.esm", "Blank.esm", "Blàñk.esp"];

        assert_eq!(expected_filenames, load_order.active_plugin_names());
    }

    #[test]
    fn load_should_ignore_plugins_in_active_plugins_file_that_are_not_installed() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        write_active_plugins_file(
            load_order.game_settings(),
            &["Blàñk.esp", "Blank.esm", "missing.esp"],
        );

        load_order.load().unwrap();
        let expected_filenames = vec!["Skyrim.esm", "Blank.esm", "Blàñk.esp"];

        assert_eq!(expected_filenames, load_order.active_plugin_names());
    }

    #[test]
    fn load_should_succeed_when_load_order_and_active_plugins_files_are_missing() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        assert!(load_order.load().is_ok());
        assert_eq!(1, load_order.active_plugin_names().len());
    }

    #[test]
    fn load_should_not_duplicate_a_plugin_that_is_ghosted_and_in_load_order_file() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        use std::fs::rename;

        rename(
            load_order
                .game_settings()
                .plugins_directory()
                .join("Blank.esm"),
            load_order
                .game_settings()
                .plugins_directory()
                .join("Blank.esm.ghost"),
        )
        .unwrap();

        let expected_filenames = vec![
            "Skyrim.esm",
            "Blank.esm",
            "Blàñk.esp",
            "Blank - Master Dependent.esp",
            "Blank - Different.esp",
            "Blank.esp",
            "missing.esp",
        ];
        write_load_order_file(load_order.game_settings(), &expected_filenames);

        load_order.load().unwrap();

        let expected_filenames = vec![
            load_order.game_settings().master_file(),
            "Blank.esm",
            "Blàñk.esp",
            "Blank - Master Dependent.esp",
            "Blank - Different.esp",
            "Blank.esp",
        ];

        assert_eq!(expected_filenames, load_order.plugin_names());
    }

    #[test]
    fn save_should_write_all_plugins_to_load_order_file() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        load_order.save().unwrap();

        let expected_filenames = vec!["Skyrim.esm", "Blank.esp", "Blank - Different.esp"];
        let plugin_names = read_utf8_plugin_names(
            load_order.game_settings().load_order_file().unwrap(),
            plugin_line_mapper,
        )
        .unwrap();
        assert_eq!(expected_filenames, plugin_names);
    }

    #[test]
    fn save_should_create_active_plugins_file_parent_directory_if_it_does_not_exist() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        remove_dir_all(
            load_order
                .game_settings()
                .active_plugins_file()
                .parent()
                .unwrap(),
        )
        .unwrap();

        load_order.save().unwrap();

        assert!(load_order
            .game_settings()
            .active_plugins_file()
            .parent()
            .unwrap()
            .exists());
    }

    #[test]
    fn save_should_write_active_plugins_file() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        load_order.save().unwrap();

        load_order.load().unwrap();
        assert_eq!(
            vec!["Skyrim.esm", "Blank.esp"],
            load_order.active_plugin_names()
        );
    }

    #[test]
    fn save_should_error_if_an_active_plugin_filename_cannot_be_encoded_in_windows_1252() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let filename = "Bl\u{0227}nk.esm";
        copy_to_test_dir(
            "Blank - Different.esm",
            filename,
            &load_order.game_settings(),
        );
        let mut plugin = Plugin::new(filename, &load_order.game_settings()).unwrap();
        plugin.activate().unwrap();
        load_order.plugins_mut().push(plugin);

        match load_order.save().unwrap_err() {
            Error::EncodeError(s) => assert_eq!("unrepresentable character", s),
            e => panic!("Expected encode error, got {:?}", e),
        };
    }

    #[test]
    fn set_load_order_should_error_if_given_an_empty_list() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let existing_filenames = to_owned(load_order.plugin_names());
        let filenames = vec![];
        assert!(load_order.set_load_order(&filenames).is_err());
        assert_eq!(existing_filenames, load_order.plugin_names());
    }

    #[test]
    fn set_load_order_should_error_if_the_first_element_given_is_not_the_game_master() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let existing_filenames = to_owned(load_order.plugin_names());
        let filenames = vec!["Blank.esp"];
        assert!(load_order.set_load_order(&filenames).is_err());
        assert_eq!(existing_filenames, load_order.plugin_names());
    }

    #[test]
    fn set_load_order_should_not_error_if_update_esm_loads_after_another_plugin() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        copy_to_test_dir("Blank.esm", "Update.esm", &load_order.game_settings());

        let filenames = vec![
            "Skyrim.esm",
            "Blank.esm",
            "Update.esm",
            "Blank.esp",
            "Blank - Master Dependent.esp",
            "Blank - Different.esp",
            "Blàñk.esp",
        ];

        assert!(load_order.set_load_order(&filenames).is_ok());
    }

    #[test]
    fn set_load_order_should_not_distinguish_between_ghosted_and_unghosted_filenames() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        copy_to_test_dir(
            "Blank - Different.esm",
            "ghosted.esm.ghost",
            &load_order.game_settings(),
        );

        let filenames = vec![
            "Skyrim.esm",
            "Blank.esm",
            "ghosted.esm",
            "Blank.esp",
            "Blank - Master Dependent.esp",
            "Blank - Different.esp",
            "Blàñk.esp",
        ];

        assert!(load_order.set_load_order(&filenames).is_ok());
    }

    #[test]
    fn set_load_order_should_not_insert_missing_plugins() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let filenames = vec![
            "Skyrim.esm",
            "Blank.esm",
            "Blank.esp",
            "Blank - Master Dependent.esp",
            "Blank - Different.esp",
        ];
        load_order.set_load_order(&filenames).unwrap();

        assert_eq!(filenames, load_order.plugin_names());
    }

    #[test]
    fn set_load_order_should_not_lose_active_state_of_existing_plugins() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let filenames = vec![
            "Skyrim.esm",
            "Blank.esm",
            "Blank.esp",
            "Blank - Master Dependent.esp",
            "Blank - Different.esp",
        ];
        load_order.set_load_order(&filenames).unwrap();

        assert!(load_order.is_active("Blank.esp"));
    }

    #[test]
    fn set_plugin_index_should_error_if_setting_the_game_master_index_to_non_zero_in_bounds() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let existing_filenames = to_owned(load_order.plugin_names());
        assert!(load_order.set_plugin_index("Skyrim.esm", 1).is_err());
        assert_eq!(existing_filenames, load_order.plugin_names());
    }

    #[test]
    fn set_plugin_index_should_error_if_setting_a_zero_index_for_a_non_game_master_plugin() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let existing_filenames = to_owned(load_order.plugin_names());
        assert!(load_order.set_plugin_index("Blank.esm", 0).is_err());
        assert_eq!(existing_filenames, load_order.plugin_names());
    }

    #[test]
    fn set_plugin_index_should_insert_a_new_plugin() {
        let tmp_dir = tempdir().unwrap();
        let mut load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let num_plugins = load_order.plugins().len();
        assert_eq!(1, load_order.set_plugin_index("Blank.esm", 1).unwrap());
        assert_eq!(1, load_order.index_of("Blank.esm").unwrap());
        assert_eq!(num_plugins + 1, load_order.plugins().len());
    }

    #[test]
    fn is_self_consistent_should_return_true_when_no_load_order_file_exists() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        assert!(load_order.is_self_consistent().unwrap());
    }

    #[test]
    fn is_self_consistent_should_return_true_when_no_active_plugins_file_exists() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let expected_filenames = vec!["Skyrim.esm", "Blank - Master Dependent.esp"];
        write_load_order_file(load_order.game_settings(), &expected_filenames);

        assert!(load_order.is_self_consistent().unwrap());
    }

    #[test]
    fn is_self_consistent_should_return_false_when_load_order_and_active_plugins_files_mismatch() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        write_active_plugins_file(
            load_order.game_settings(),
            &["Blàñk.esp", "Blank.esm", "missing.esp"],
        );

        let expected_filenames = vec!["Blàñk.esp", "missing.esp", "Blank.esm\r"];
        write_load_order_file(load_order.game_settings(), &expected_filenames);

        assert!(!load_order.is_self_consistent().unwrap());
    }

    #[test]
    fn is_self_consistent_should_return_true_when_load_order_and_active_plugins_files_match() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        write_active_plugins_file(
            load_order.game_settings(),
            &["Blàñk.esp", "Blank.esm", "missing.esp"],
        );

        // loadorder.txt should be a case-insensitive sorted superset of plugins.txt.
        let expected_filenames = vec!["Skyrim.esm", "Blàñk.esp", "Blank.esm\r", "missing.esp"];
        write_load_order_file(load_order.game_settings(), &expected_filenames);

        assert!(load_order.is_self_consistent().unwrap());
    }

    #[test]
    fn is_self_consistent_should_read_load_order_file_as_windows_1252_if_not_utf8() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        write_active_plugins_file(
            load_order.game_settings(),
            &["Blàñk.esp", "Blank.esm", "missing.esp"],
        );

        // loadorder.txt should be a case-insensitive sorted superset of plugins.txt.
        let expected_filenames = vec!["Skyrim.esm", "Blàñk.esp", "Blank.esm\r", "missing.esp"];

        let mut file =
            File::create(&load_order.game_settings().load_order_file().unwrap()).unwrap();

        for filename in &expected_filenames {
            file.write_all(&strict_encode(filename).unwrap()).unwrap();
            writeln!(file).unwrap();
        }

        assert!(load_order.is_self_consistent().unwrap());
    }

    #[test]
    fn is_ambiguous_should_return_true_if_load_order_is_not_self_consistent() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        write_active_plugins_file(
            load_order.game_settings(),
            &["Blàñk.esp", "Blank.esm", "missing.esp"],
        );

        let expected_filenames = vec!["Blàñk.esp", "missing.esp", "Blank.esm\r"];
        write_load_order_file(load_order.game_settings(), &expected_filenames);

        assert!(!load_order.is_self_consistent().unwrap());
        assert!(load_order.is_ambiguous().unwrap());
    }

    #[test]
    fn is_ambiguous_should_return_true_if_active_plugins_and_load_order_files_do_not_exist() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        assert!(load_order.is_ambiguous().unwrap());
    }

    #[test]
    fn is_ambiguous_should_return_true_if_only_active_plugins_file_exists_and_does_not_list_all_loaded_plugins(
    ) {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let mut loaded_plugin_names: Vec<&str> = load_order
            .plugins
            .iter()
            .map(|plugin| plugin.name())
            .collect();

        loaded_plugin_names.pop();

        write_active_plugins_file(load_order.game_settings(), &loaded_plugin_names);

        assert!(load_order.is_ambiguous().unwrap());
    }

    #[test]
    fn is_ambiguous_should_return_false_if_only_active_plugins_file_exists_and_lists_all_loaded_plugins(
    ) {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let loaded_plugin_names: Vec<&str> = load_order
            .plugins
            .iter()
            .map(|plugin| plugin.name())
            .collect();

        write_active_plugins_file(load_order.game_settings(), &loaded_plugin_names);

        assert!(!load_order.is_ambiguous().unwrap());
    }

    #[test]
    fn is_ambiguous_should_return_true_if_only_load_order_file_exists_and_does_not_list_all_loaded_plugins(
    ) {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let mut loaded_plugin_names: Vec<&str> = load_order
            .plugins
            .iter()
            .map(|plugin| plugin.name())
            .collect();

        loaded_plugin_names.pop();

        write_load_order_file(load_order.game_settings(), &loaded_plugin_names);

        assert!(load_order.is_ambiguous().unwrap());
    }

    #[test]
    fn is_ambiguous_should_return_false_if_only_load_order_file_exists_and_lists_all_loaded_plugins(
    ) {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let loaded_plugin_names: Vec<&str> = load_order
            .plugins
            .iter()
            .map(|plugin| plugin.name())
            .collect();

        write_load_order_file(load_order.game_settings(), &loaded_plugin_names);

        assert!(!load_order.is_ambiguous().unwrap());
    }

    #[test]
    fn is_ambiguous_should_read_load_order_file_as_windows_1252_if_not_utf8() {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let loaded_plugin_names: Vec<&str> = load_order
            .plugins
            .iter()
            .map(|plugin| plugin.name())
            .collect();

        let mut file =
            File::create(&load_order.game_settings().load_order_file().unwrap()).unwrap();

        for filename in &loaded_plugin_names {
            file.write_all(&strict_encode(filename).unwrap()).unwrap();
            writeln!(file).unwrap();
        }

        assert!(!load_order.is_ambiguous().unwrap());
    }

    #[test]
    fn is_ambiguous_should_return_true_if_active_plugins_and_load_order_files_exist_and_load_order_file_does_not_list_all_loaded_plugins(
    ) {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let mut loaded_plugin_names: Vec<&str> = load_order
            .plugins
            .iter()
            .map(|plugin| plugin.name())
            .collect();

        loaded_plugin_names.pop();

        write_load_order_file(load_order.game_settings(), &loaded_plugin_names);
        write_active_plugins_file(load_order.game_settings(), &loaded_plugin_names);

        assert!(load_order.is_ambiguous().unwrap());
    }

    #[test]
    fn is_ambiguous_should_return_false_if_active_plugins_and_load_order_files_exist_and_load_order_file_lists_all_loaded_plugins(
    ) {
        let tmp_dir = tempdir().unwrap();
        let load_order = prepare(GameId::Skyrim, &tmp_dir.path());

        let mut loaded_plugin_names: Vec<&str> = load_order
            .plugins
            .iter()
            .map(|plugin| plugin.name())
            .collect();

        write_load_order_file(load_order.game_settings(), &loaded_plugin_names);

        loaded_plugin_names.pop();

        write_active_plugins_file(load_order.game_settings(), &loaded_plugin_names);

        assert!(!load_order.is_ambiguous().unwrap());
    }
}
