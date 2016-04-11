/*  libloadorder

A library for reading and writing the load order of plugin files for
TES III: Morrowind, TES IV: Oblivion, TES V: Skyrim, Fallout 3,
Fallout: New Vegas and Fallout 4.

Copyright (C) 2015 Oliver Hamlet

This file is part of libloadorder.

libloadorder is free software: you can redistribute
it and/or modify it under the terms of the GNU General Public License
as published by the Free Software Foundation, either version 3 of
the License, or (at your option) any later version.

libloadorder is distributed in the hope that it will
be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with libloadorder.  If not, see
<http://www.gnu.org/licenses/>.
*/

#include "backend/GameSettings.h"
#include "backend/LoadOrder.h"
#include "backend/helpers.h"
#include "libloadorder/constants.h"
#include "tests/GameTest.h"

#include <thread>
#include <chrono>

#include <boost/algorithm/string.hpp>

namespace liblo {
    namespace test {
        class LoadOrderTest : public GameTest {
        protected:
            inline LoadOrderTest() :
                blankMasterDependentEsm("Blank - Master Dependent.esm"),
                blankDifferentMasterDependentEsm("Blank - Different Master Dependent.esm"),
                blankEsp("Blank.esp"),
                blankDifferentEsp("Blank - Different.esp"),
                blankMasterDependentEsp("Blank - Master Dependent.esp"),
                blankDifferentMasterDependentEsp("Blank - Different Master Dependent.esp"),
                blankPluginDependentEsp("Blank - Plugin Dependent.esp"),
                blankDifferentPluginDependentEsp("Blank - Different Plugin Dependent.esp"),
                missingPlugin("missing.esm"),
                updateEsm("Update.esm"),
                nonAsciiEsm("Blàñk.esm"),
                gameSettings(GetParam(), gamePath, localPath),
                loadOrder(gameSettings) {}

            inline virtual void SetUp() {
                GameTest::SetUp();

                ASSERT_TRUE(boost::filesystem::exists(pluginsPath / blankMasterDependentEsm));
                ASSERT_TRUE(boost::filesystem::exists(pluginsPath / blankDifferentMasterDependentEsm));
                ASSERT_TRUE(boost::filesystem::exists(pluginsPath / blankEsp));
                ASSERT_TRUE(boost::filesystem::exists(pluginsPath / blankDifferentEsp));
                ASSERT_TRUE(boost::filesystem::exists(pluginsPath / blankMasterDependentEsp));
                ASSERT_TRUE(boost::filesystem::exists(pluginsPath / blankDifferentMasterDependentEsp));
                ASSERT_TRUE(boost::filesystem::exists(pluginsPath / blankDifferentPluginDependentEsp));
                ASSERT_FALSE(boost::filesystem::exists(pluginsPath / missingPlugin));

                // Make sure Update.esm exists.
                ASSERT_FALSE(boost::filesystem::exists(pluginsPath / updateEsm));
                ASSERT_NO_THROW(boost::filesystem::copy_file(pluginsPath / blankEsm, pluginsPath / updateEsm));
                ASSERT_TRUE(boost::filesystem::exists(pluginsPath / updateEsm));

                // Make sure the non-ASCII plugin exists.
                ASSERT_FALSE(boost::filesystem::exists(pluginsPath / nonAsciiEsm));
                ASSERT_NO_THROW(boost::filesystem::copy_file(pluginsPath / blankEsm, pluginsPath / nonAsciiEsm));
                ASSERT_TRUE(boost::filesystem::exists(pluginsPath / nonAsciiEsm));

                // Morrowind load order files have a slightly different
                // format and a prefix is necessary.
                std::string linePrefix = getActivePluginsFileLinePrefix();

                // Write out a load order, making it as invalid as possible
                // for the game to still fix.
                std::vector<std::pair<std::string, bool>> plugins({
                    {nonAsciiEsm, true},
                    {masterFile, false},
                    {blankDifferentEsm, false},
                    {blankEsm, true},
                    {blankMasterDependentEsm, false},
                    {blankDifferentMasterDependentEsm, false},
                    {blankEsp, true},  // Put a plugin before master to test fixup.
                    {updateEsm, false},
                    {blankDifferentEsp, false},
                    {blankMasterDependentEsp, false},
                    {blankDifferentMasterDependentEsp, false},
                    {blankPluginDependentEsp, false},
                    {blankDifferentPluginDependentEsp, false},
                    {invalidPlugin, false},
                });
                writeLoadOrder(plugins);
            }

            inline virtual void TearDown() {
                GameTest::TearDown();

                ASSERT_NO_THROW(boost::filesystem::remove(pluginsPath / updateEsm));
                ASSERT_NO_THROW(boost::filesystem::remove(pluginsPath / nonAsciiEsm));
            }

            inline void writeLoadOrder(std::vector<std::pair<std::string, bool>> loadOrder) const {
                std::string linePrefix = getActivePluginsFileLinePrefix();

                if (loadOrderMethod == LIBLO_METHOD_ASTERISK) {
                    boost::filesystem::ofstream out(activePluginsFilePath);
                    for (const auto& plugin : loadOrder) {
                        if (plugin.second)
                            out << linePrefix;

                        out << utf8ToWindows1252(plugin.first) << std::endl;
                    }
                }
                else {
                    boost::filesystem::ofstream out(activePluginsFilePath);
                    for (const auto& plugin : loadOrder) {
                        if (plugin.second)
                            out << linePrefix << utf8ToWindows1252(plugin.first) << std::endl;
                    }
                    out.close();

                    if (loadOrderMethod == LIBLO_METHOD_TEXTFILE) {
                        boost::filesystem::ofstream out(loadOrderFilePath);
                        for (const auto& plugin : loadOrder)
                            out << plugin.first << std::endl;
                    }
                    else {
                        time_t modificationTime = time(NULL);  // Current time.
                        for (const auto& plugin : loadOrder) {
                            boost::filesystem::last_write_time(pluginsPath / plugin.first, modificationTime);
                            modificationTime += 60;
                        }
                    }
                }
            }

            void incrementModTime(const boost::filesystem::path& file) {
                time_t currentModTime = boost::filesystem::last_write_time(file);
                boost::filesystem::last_write_time(file, currentModTime + 1);
            }

            void decrementModTime(const boost::filesystem::path& file) {
                time_t currentModTime = boost::filesystem::last_write_time(file);
                boost::filesystem::last_write_time(file, currentModTime - 1);
            }

            const GameSettings gameSettings;
            LoadOrder loadOrder;

            std::string blankMasterDependentEsm;
            std::string blankDifferentMasterDependentEsm;
            std::string blankEsp;
            std::string blankDifferentEsp;
            std::string blankMasterDependentEsp;
            std::string blankDifferentMasterDependentEsp;
            std::string blankPluginDependentEsp;
            std::string blankDifferentPluginDependentEsp;

            std::string missingPlugin;
            std::string updateEsm;
            std::string nonAsciiEsm;
        };

        // Pass an empty first argument, as it's a prefix for the test instantation,
        // but we only have the one so no prefix is necessary.
        INSTANTIATE_TEST_CASE_P(,
                                LoadOrderTest,
                                ::testing::Values(
                                    LIBLO_GAME_TES3,
                                    LIBLO_GAME_TES4,
                                    LIBLO_GAME_TES5,
                                    LIBLO_GAME_FO3,
                                    LIBLO_GAME_FNV,
                                    LIBLO_GAME_FO4));

        TEST_P(LoadOrderTest, settingAValidLoadOrderShouldNotThrow) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            EXPECT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));
        }

        TEST_P(LoadOrderTest, settingALoadOrderWithPluginsBeforeMastersShouldThrow) {
            std::vector<std::string> invalidLoadOrder({
                masterFile,
                blankEsp,
                blankDifferentEsm,
            });
            EXPECT_ANY_THROW(loadOrder.setLoadOrder(invalidLoadOrder));
        }

        TEST_P(LoadOrderTest, settingALoadOrderWithPluginsBeforeMastersShouldMakeNoChanges) {
            std::vector<std::string> invalidLoadOrder({
                masterFile,
                blankEsp,
                blankDifferentEsm,
            });
            EXPECT_ANY_THROW(loadOrder.setLoadOrder(invalidLoadOrder));
            EXPECT_TRUE(loadOrder.getLoadOrder().empty());
        }

        TEST_P(LoadOrderTest, settingALoadOrderWithAnInvalidPluginShouldThrow) {
            std::vector<std::string> invalidLoadOrder({
                masterFile,
                invalidPlugin,
            });
            EXPECT_ANY_THROW(loadOrder.setLoadOrder(invalidLoadOrder));
        }

        TEST_P(LoadOrderTest, settingALoadOrderWithAnInvalidPluginShouldMakeNoChanges) {
            std::vector<std::string> invalidLoadOrder({
                masterFile,
                invalidPlugin,
            });
            EXPECT_ANY_THROW(loadOrder.setLoadOrder(invalidLoadOrder));
            EXPECT_TRUE(loadOrder.getLoadOrder().empty());
        }

        TEST_P(LoadOrderTest, settingALoadOrderWithACaseInsensitiveDuplicatePluginShouldThrow) {
            std::vector<std::string> invalidLoadOrder({
                masterFile,
                blankEsm,
                boost::to_lower_copy(blankEsm),
            });
            EXPECT_ANY_THROW(loadOrder.setLoadOrder(invalidLoadOrder));
        }

        TEST_P(LoadOrderTest, settingALoadOrderWithACaseInsensitiveDuplicatePluginShouldMakeNoChanges) {
            std::vector<std::string> invalidLoadOrder({
                masterFile,
                blankEsm,
                boost::to_lower_copy(blankEsm),
            });
            EXPECT_ANY_THROW(loadOrder.setLoadOrder(invalidLoadOrder));
            EXPECT_TRUE(loadOrder.getLoadOrder().empty());
        }

        TEST_P(LoadOrderTest, settingThenGettingLoadOrderShouldReturnTheSetLoadOrder) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            EXPECT_TRUE(std::equal(begin(validLoadOrder), end(validLoadOrder), begin(loadOrder.getLoadOrder())));
        }

        TEST_P(LoadOrderTest, settingTheLoadOrderTwiceShouldReplaceTheFirstLoadOrder) {
            std::vector<std::string> firstLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            std::vector<std::string> secondLoadOrder({
                masterFile,
                blankDifferentEsm,
                blankEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(firstLoadOrder));
            ASSERT_NO_THROW(loadOrder.setLoadOrder(secondLoadOrder));

            EXPECT_TRUE(std::equal(begin(secondLoadOrder), end(secondLoadOrder), begin(loadOrder.getLoadOrder())));
        }

        TEST_P(LoadOrderTest, settingAnInvalidLoadOrderShouldMakeNoChanges) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            std::vector<std::string> invalidLoadOrder({
                masterFile,
                blankEsp,
                blankDifferentEsm,
            });

            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));
            EXPECT_ANY_THROW(loadOrder.setLoadOrder(invalidLoadOrder));

            EXPECT_TRUE(std::equal(begin(validLoadOrder), end(validLoadOrder), begin(loadOrder.getLoadOrder())));
        }

        TEST_P(LoadOrderTest, settingALoadOrderWithTheGameMasterNotAtTheBeginningShouldFailForTextfileAndAsteriskLoadOrderGamesAndSucceedOtherwise) {
            std::vector<std::string> plugins({
                blankEsm,
                masterFile,
            });
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK)
                EXPECT_ANY_THROW(loadOrder.setLoadOrder(plugins));
            else
                EXPECT_NO_THROW(loadOrder.setLoadOrder(plugins));
        }

        TEST_P(LoadOrderTest, settingALoadOrderWithTheGameMasterNotAtTheBeginningShouldMakeNoChangesForTextfileAndAsteriskLoadOrderGames) {
            std::vector<std::string> plugins({
                blankEsm,
                masterFile,
            });
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK) {
                EXPECT_ANY_THROW(loadOrder.setLoadOrder(plugins));
                EXPECT_TRUE(loadOrder.getLoadOrder().empty());
            }
        }

        TEST_P(LoadOrderTest, positionOfAMissingPluginShouldEqualTheLoadOrderSize) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            EXPECT_EQ(13, loadOrder.getPosition(missingPlugin));
        }

        TEST_P(LoadOrderTest, positionOfAPluginShouldBeEqualToItsLoadOrderIndex) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            EXPECT_EQ(1, loadOrder.getPosition(blankEsm));
        }

        TEST_P(LoadOrderTest, gettingAPluginsPositionShouldBeCaseInsensitive) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            EXPECT_EQ(1, loadOrder.getPosition(boost::to_lower_copy(blankEsm)));
        }

        TEST_P(LoadOrderTest, gettingPluginAtAPositionGreaterThanTheHighestIndexShouldThrow) {
            EXPECT_ANY_THROW(loadOrder.getPluginAtPosition(0));
        }

        TEST_P(LoadOrderTest, gettingPluginAtAValidPositionShouldReturnItsLoadOrderIndex) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            EXPECT_EQ(blankEsm, loadOrder.getPluginAtPosition(1));
        }

        TEST_P(LoadOrderTest, settingAPluginThatIsNotTheGameMasterFileToLoadFirstShouldThrowForTextfileAndAsteriskLoadOrderGamesAndNotOtherwise) {
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK)
                EXPECT_ANY_THROW(loadOrder.setPosition(blankEsm, 0));
            else {
                EXPECT_NO_THROW(loadOrder.setPosition(blankEsm, 0));
            }
        }

        TEST_P(LoadOrderTest, settingAPluginThatIsNotTheGameMasterFileToLoadFirstForATextfileOrAsteriskBasedGameShouldMakeNoChanges) {
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK) {
                EXPECT_ANY_THROW(loadOrder.setPosition(blankEsm, 0));
                EXPECT_TRUE(loadOrder.getLoadOrder().empty());
            }
        }

        TEST_P(LoadOrderTest, settingAPluginThatIsNotTheGameMasterFileToLoadFirstForATimestampOrAsteriskBasedGameShouldSucceed) {
            if (loadOrderMethod == LIBLO_METHOD_TIMESTAMP) {
                EXPECT_NO_THROW(loadOrder.setPosition(blankEsm, 0));
                EXPECT_FALSE(loadOrder.getLoadOrder().empty());
                EXPECT_EQ(0, loadOrder.getPosition(blankEsm));
            }
        }

        TEST_P(LoadOrderTest, settingTheGameMasterFileToLoadAfterAnotherPluginShouldThrowForTextfileAndAsteriskLoadOrderGamesAndNotOtherwise) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK)
                EXPECT_ANY_THROW(loadOrder.setPosition(masterFile, 1));
            else
                EXPECT_NO_THROW(loadOrder.setPosition(masterFile, 1));
        }

        TEST_P(LoadOrderTest, settingTheGameMasterFileToLoadAfterAnotherPluginShouldMakeNoChangesForTextfileOrAsteriskLoadOrderGames) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK) {
                EXPECT_ANY_THROW(loadOrder.setPosition(masterFile, 1));
                EXPECT_EQ(0, loadOrder.getPosition(masterFile));
            }
        }

        TEST_P(LoadOrderTest, settingTheGameMasterFileToLoadAfterAnotherPluginForATextfileBasedGameShouldMakeNoChanges) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE) {
                ASSERT_ANY_THROW(loadOrder.setPosition(masterFile, 1));
                EXPECT_EQ(blankEsm, loadOrder.getPluginAtPosition(1));
            }
        }

        TEST_P(LoadOrderTest, settingTheGameMasterFileToLoadAfterAnotherPluginForATimestampBasedGameShouldSucceed) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            if (loadOrderMethod == LIBLO_METHOD_TIMESTAMP) {
                ASSERT_NO_THROW(loadOrder.setPosition(masterFile, 1));
                EXPECT_EQ(blankEsm, loadOrder.getPluginAtPosition(0));
                EXPECT_EQ(masterFile, loadOrder.getPluginAtPosition(1));
            }
        }

        TEST_P(LoadOrderTest, settingThePositionOfAnInvalidPluginShouldThrow) {
            ASSERT_NO_THROW(loadOrder.setPosition(masterFile, 0));

            EXPECT_ANY_THROW(loadOrder.setPosition(invalidPlugin, 1));
        }

        TEST_P(LoadOrderTest, settingThePositionOfAnInvalidPluginShouldMakeNoChanges) {
            ASSERT_NO_THROW(loadOrder.setPosition(masterFile, 0));

            ASSERT_ANY_THROW(loadOrder.setPosition(invalidPlugin, 1));
            EXPECT_EQ(1, loadOrder.getLoadOrder().size());
        }

        TEST_P(LoadOrderTest, settingThePositionOfAPluginToGreaterThanTheLoadOrderSizeShouldPutThePluginAtTheEnd) {
            ASSERT_NO_THROW(loadOrder.setPosition(masterFile, 0));

            EXPECT_NO_THROW(loadOrder.setPosition(blankEsm, 2));
            EXPECT_EQ(2, loadOrder.getLoadOrder().size());
            EXPECT_EQ(1, loadOrder.getPosition(blankEsm));
        }

        TEST_P(LoadOrderTest, settingThePositionOfAPluginShouldBeCaseInsensitive) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            EXPECT_NO_THROW(loadOrder.setPosition(boost::to_lower_copy(blankEsm), 2));

            std::vector<std::string> expectedLoadOrder({
                masterFile,
                blankDifferentEsm,
                blankEsm,
            });

            EXPECT_TRUE(std::equal(begin(expectedLoadOrder), end(expectedLoadOrder), begin(loadOrder.getLoadOrder())));
        }

        TEST_P(LoadOrderTest, settingANonMasterPluginToLoadBeforeAMasterPluginShouldThrow) {
            ASSERT_NO_THROW(loadOrder.load());

            EXPECT_ANY_THROW(loadOrder.setPosition(blankEsp, 1));
        }

        TEST_P(LoadOrderTest, settingANonMasterPluginToLoadBeforeAMasterPluginShouldMakeNoChanges) {
            ASSERT_NO_THROW(loadOrder.load());

            EXPECT_ANY_THROW(loadOrder.setPosition(blankEsp, 1));
            EXPECT_NE(1, loadOrder.getPosition(blankEsp));
        }

        TEST_P(LoadOrderTest, settingAMasterToLoadAfterAPluginShouldThrow) {
            ASSERT_NO_THROW(loadOrder.load());

            EXPECT_ANY_THROW(loadOrder.setPosition(blankEsm, 10));
        }

        TEST_P(LoadOrderTest, settingAMasterToLoadAfterAPluginShouldMakeNoChanges) {
            ASSERT_NO_THROW(loadOrder.load());

            EXPECT_ANY_THROW(loadOrder.setPosition(blankEsm, 10));
            EXPECT_NE(10, loadOrder.getPosition(blankEsm));
        }

        TEST_P(LoadOrderTest, clearingLoadOrderShouldRemoveAllPluginsFromTheLoadOrder) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankEsp,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            EXPECT_NO_THROW(loadOrder.clear());
            EXPECT_TRUE(loadOrder.getLoadOrder().empty());
        }

        TEST_P(LoadOrderTest, clearingLoadOrderShouldResetTimestamps) {
            ASSERT_NO_THROW(loadOrder.load());

            EXPECT_NO_THROW(loadOrder.clear());
            ASSERT_NO_THROW(loadOrder.load());
            EXPECT_FALSE(loadOrder.getLoadOrder().empty());
        }

        TEST_P(LoadOrderTest, checkingIfAnInactivePluginIsActiveShouldReturnFalse) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            EXPECT_FALSE(loadOrder.isActive(blankEsm));
        }

        TEST_P(LoadOrderTest, checkingIfAPluginNotInTheLoadOrderIsActiveShouldReturnFalse) {
            EXPECT_FALSE(loadOrder.isActive(blankEsp));
        }

        TEST_P(LoadOrderTest, activatingAnInvalidPluginShouldThrow) {
            EXPECT_ANY_THROW(loadOrder.activate(invalidPlugin));
        }

        TEST_P(LoadOrderTest, activatingANonMasterPluginNotInTheLoadOrderShouldAppendItToTheLoadOrder) {
            ASSERT_NO_THROW(loadOrder.setPosition(masterFile, 0));

            EXPECT_NO_THROW(loadOrder.activate(blankEsp));
            EXPECT_EQ(1, loadOrder.getPosition(blankEsp));
            EXPECT_TRUE(loadOrder.isActive(blankEsp));
        }

        TEST_P(LoadOrderTest, activatingAMasterPluginNotInTheLoadOrderShouldInsertItAfterAllOtherMasters) {
            ASSERT_NO_THROW(loadOrder.setPosition(masterFile, 0));
            ASSERT_NO_THROW(loadOrder.setPosition(blankEsp, 1));

            EXPECT_NO_THROW(loadOrder.activate(blankDifferentEsm));
            EXPECT_EQ(1, loadOrder.getPosition(blankDifferentEsm));
            EXPECT_TRUE(loadOrder.isActive(blankDifferentEsm));
        }

        TEST_P(LoadOrderTest, activatingTheGameMasterFileNotInTheLoadOrderShouldInsertItAfterAllOtherMastersForTimestampBasedGamesAndAtTheBeginningOtherwise) {
            ASSERT_NO_THROW(loadOrder.activate(blankEsm));

            EXPECT_NO_THROW(loadOrder.activate(masterFile));
            if (loadOrderMethod == LIBLO_METHOD_TIMESTAMP)
                EXPECT_EQ(1, loadOrder.getPosition(masterFile));
            else
                EXPECT_EQ(0, loadOrder.getPosition(masterFile));
        }

        TEST_P(LoadOrderTest, activatingAPluginInTheLoadOrderShouldSetItToActive) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));
            ASSERT_FALSE(loadOrder.isActive(blankDifferentEsm));

            EXPECT_NO_THROW(loadOrder.activate(blankDifferentEsm));
            EXPECT_TRUE(loadOrder.isActive(blankDifferentEsm));
        }

        TEST_P(LoadOrderTest, checkingIfAPluginIsActiveShouldBeCaseInsensitive) {
            EXPECT_NO_THROW(loadOrder.activate(blankEsm));
            EXPECT_TRUE(loadOrder.isActive(boost::to_lower_copy(blankEsm)));
        }

        TEST_P(LoadOrderTest, activatingAPluginShouldBeCaseInsensitive) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            EXPECT_NO_THROW(loadOrder.activate(boost::to_lower_copy(blankEsm)));

            EXPECT_TRUE(loadOrder.isActive(blankEsm));

            EXPECT_TRUE(std::equal(begin(validLoadOrder), end(validLoadOrder), begin(loadOrder.getLoadOrder())));
        }

        TEST_P(LoadOrderTest, activatingAPluginWhenMaxNumberAreAlreadyActiveShouldThrow) {
            // Create plugins to test active plugins limit with. Do it
            // here because it's too expensive to do for every test.
            for (size_t i = 0; i < LoadOrder::maxActivePlugins; ++i) {
                EXPECT_NO_THROW(boost::filesystem::copy_file(pluginsPath / blankEsp, pluginsPath / (std::to_string(i) + ".esp")));
                EXPECT_NO_THROW(loadOrder.activate(std::to_string(i) + ".esp"));
            }

            EXPECT_ANY_THROW(loadOrder.activate(blankEsm));

            for (size_t i = 0; i < LoadOrder::maxActivePlugins; ++i)
                EXPECT_NO_THROW(boost::filesystem::remove(pluginsPath / (std::to_string(i) + ".esp")));
        }

        TEST_P(LoadOrderTest, activatingAPluginWhenMaxNumberAreAlreadyActiveShouldMakeNoChanges) {
            // Create plugins to test active plugins limit with. Do it
            // here because it's too expensive to do for every test.
            for (size_t i = 0; i < LoadOrder::maxActivePlugins; ++i) {
                EXPECT_NO_THROW(boost::filesystem::copy_file(pluginsPath / blankEsp, pluginsPath / (std::to_string(i) + ".esp")));
                EXPECT_NO_THROW(loadOrder.activate(std::to_string(i) + ".esp"));
            }

            EXPECT_ANY_THROW(loadOrder.activate(blankEsm));
            EXPECT_FALSE(loadOrder.isActive(blankEsm));

            for (size_t i = 0; i < LoadOrder::maxActivePlugins; ++i)
                EXPECT_NO_THROW(boost::filesystem::remove(pluginsPath / (std::to_string(i) + ".esp")));
        }

        TEST_P(LoadOrderTest, deactivatingAPluginNotInTheLoadOrderShouldDoNothing) {
            EXPECT_NO_THROW(loadOrder.deactivate(blankEsp));
            EXPECT_FALSE(loadOrder.isActive(blankEsp));
            EXPECT_TRUE(loadOrder.getLoadOrder().empty());
        }

        TEST_P(LoadOrderTest, deactivatingTheGameMasterFileShouldThrowForTextfileAndAsteriskLoadOrderGamesAndNotOtherwise) {
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK)
                EXPECT_ANY_THROW(loadOrder.deactivate(masterFile));
            else
                EXPECT_NO_THROW(loadOrder.deactivate(masterFile));
        }

        TEST_P(LoadOrderTest, deactivatingTheGameMasterFileShouldThrowAndMakeNoChangesForTextfileAndAsteriskLoadOrderGames) {
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK) {
                EXPECT_ANY_THROW(loadOrder.deactivate(masterFile));
                EXPECT_FALSE(loadOrder.isActive(masterFile));
            }
        }

        TEST_P(LoadOrderTest, forSkyrimDeactivatingUpdateEsmShouldThrow) {
            if (GetParam() == LIBLO_GAME_TES5)
                EXPECT_ANY_THROW(loadOrder.deactivate(updateEsm));
        }

        TEST_P(LoadOrderTest, forSkyrimDeactivatingUpdateEsmShouldMakeNoChanges) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                updateEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));
            ASSERT_NO_THROW(loadOrder.activate(updateEsm));

            if (GetParam() == LIBLO_GAME_TES5) {
                EXPECT_ANY_THROW(loadOrder.deactivate(updateEsm));
                EXPECT_TRUE(loadOrder.isActive(updateEsm));
            }
        }

        TEST_P(LoadOrderTest, deactivatingAnInactivePluginShouldHaveNoEffect) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));
            ASSERT_FALSE(loadOrder.isActive(blankEsm));

            EXPECT_NO_THROW(loadOrder.deactivate(blankEsm));
            EXPECT_FALSE(loadOrder.isActive(blankEsm));
        }

        TEST_P(LoadOrderTest, deactivatingAnActivePluginShouldMakeItInactive) {
            ASSERT_NO_THROW(loadOrder.activate(blankEsp));
            ASSERT_TRUE(loadOrder.isActive(blankEsp));

            EXPECT_NO_THROW(loadOrder.deactivate(blankEsp));
            EXPECT_FALSE(loadOrder.isActive(blankEsp));
        }

        TEST_P(LoadOrderTest, settingThePositionOfAnActivePluginShouldKeepItActive) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));
            ASSERT_NO_THROW(loadOrder.activate(blankEsm));

            loadOrder.setPosition(blankEsm, 2);
            EXPECT_TRUE(loadOrder.isActive(blankEsm));
        }

        TEST_P(LoadOrderTest, settingThePositionOfAnInactivePluginShouldKeepItInactive) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));

            loadOrder.setPosition(blankEsm, 2);
            EXPECT_FALSE(loadOrder.isActive(blankEsm));
        }

        TEST_P(LoadOrderTest, settingLoadOrderShouldActivateTheGameMasterForTextfileAndAsteriskBasedGamesAndNotOtherwise) {
            std::vector<std::string> firstLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(firstLoadOrder));
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK)
                EXPECT_TRUE(loadOrder.isActive(masterFile));
            else
                EXPECT_FALSE(loadOrder.isActive(masterFile));
        }

        TEST_P(LoadOrderTest, settingANewLoadOrderShouldRetainTheActiveStateOfPluginsInTheOldLoadOrder) {
            std::vector<std::string> firstLoadOrder({
                masterFile,
                blankEsm,
                blankDifferentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(firstLoadOrder));
            ASSERT_NO_THROW(loadOrder.activate(blankEsm));

            std::vector<std::string> secondLoadOrder({
                masterFile,
                blankEsm,
                blankEsp,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(secondLoadOrder));

            EXPECT_TRUE(loadOrder.isActive(blankEsm));
            EXPECT_FALSE(loadOrder.isActive(blankEsp));
        }

        TEST_P(LoadOrderTest, settingInvalidActivePluginsShouldThrow) {
            std::unordered_set<std::string> activePlugins({
                masterFile,
                updateEsm,
                invalidPlugin,
            });
            EXPECT_ANY_THROW(loadOrder.setActivePlugins(activePlugins));
        }

        TEST_P(LoadOrderTest, settingInvalidActivePluginsShouldMakeNoChanges) {
            std::unordered_set<std::string> activePlugins({
                masterFile,
                updateEsm,
                invalidPlugin,
            });
            EXPECT_ANY_THROW(loadOrder.setActivePlugins(activePlugins));
            EXPECT_TRUE(loadOrder.getActivePlugins().empty());
        }

        TEST_P(LoadOrderTest, settingMoreThanMaxNumberActivePluginsShouldThrow) {
            // Create plugins to test active plugins limit with. Do it
            // here because it's too expensive to do for every test.
            std::unordered_set<std::string> activePlugins({
                masterFile,
                updateEsm,
            });
            for (size_t i = 0; i < LoadOrder::maxActivePlugins; ++i) {
                EXPECT_NO_THROW(boost::filesystem::copy_file(pluginsPath / blankEsp, pluginsPath / (std::to_string(i) + ".esp")));
                activePlugins.insert(std::to_string(i) + ".esp");
            }

            EXPECT_ANY_THROW(loadOrder.setActivePlugins(activePlugins));

            for (size_t i = 0; i < LoadOrder::maxActivePlugins; ++i)
                EXPECT_NO_THROW(boost::filesystem::remove(pluginsPath / (std::to_string(i) + ".esp")));
        }

        TEST_P(LoadOrderTest, settingMoreThanMaxNumberActivePluginsShouldMakeNoChanges) {
            // Create plugins to test active plugins limit with. Do it
            // here because it's too expensive to do for every test.
            std::unordered_set<std::string> activePlugins({
                masterFile,
                updateEsm,
            });
            for (size_t i = 0; i < LoadOrder::maxActivePlugins; ++i) {
                EXPECT_NO_THROW(boost::filesystem::copy_file(pluginsPath / blankEsp, pluginsPath / (std::to_string(i) + ".esp")));
                activePlugins.insert(std::to_string(i) + ".esp");
            }

            EXPECT_ANY_THROW(loadOrder.setActivePlugins(activePlugins));
            EXPECT_TRUE(loadOrder.getActivePlugins().empty());

            for (size_t i = 0; i < LoadOrder::maxActivePlugins; ++i)
                EXPECT_NO_THROW(boost::filesystem::remove(pluginsPath / (std::to_string(i) + ".esp")));
        }

        TEST_P(LoadOrderTest, settingActivePluginsWithoutGameMasterShouldThrowForTextfileAndAsteriskBasedGamesAndNotOtherwise) {
            std::unordered_set<std::string> activePlugins({
                updateEsm,
                blankEsm,
            });
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK)
                EXPECT_ANY_THROW(loadOrder.setActivePlugins(activePlugins));
            else
                EXPECT_NO_THROW(loadOrder.setActivePlugins(activePlugins));
        }

        TEST_P(LoadOrderTest, settingActivePluginsWithoutGameMasterShouldMakeNoChangesForTextfileAndAsteriskBasedGames) {
            std::unordered_set<std::string> activePlugins({
                updateEsm,
                blankEsm,
            });
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK) {
                EXPECT_ANY_THROW(loadOrder.setActivePlugins(activePlugins));
                EXPECT_TRUE(loadOrder.getActivePlugins().empty());
            }
        }

        TEST_P(LoadOrderTest, settingActivePluginsWithoutUpdateEsmWhenItExistsShouldThrowForSkyrimAndNotOtherwise) {
            std::unordered_set<std::string> activePlugins({
                masterFile,
                blankEsm,
            });
            if (GetParam() == LIBLO_GAME_TES5)
                EXPECT_ANY_THROW(loadOrder.setActivePlugins(activePlugins));
            else
                EXPECT_NO_THROW(loadOrder.setActivePlugins(activePlugins));
        }

        TEST_P(LoadOrderTest, settingActivePluginsWithoutUpdateEsmWhenItExistsShouldMakeNoChangesForSkyrim) {
            std::unordered_set<std::string> activePlugins({
                masterFile,
                blankEsm,
            });
            if (GetParam() == LIBLO_GAME_TES5) {
                EXPECT_ANY_THROW(loadOrder.setActivePlugins(activePlugins));
                EXPECT_TRUE(loadOrder.getActivePlugins().empty());
            }
        }

        TEST_P(LoadOrderTest, settingActivePluginsWithoutUpdateEsmWhenItDoesNotExistShouldNotThrow) {
            ASSERT_NO_THROW(boost::filesystem::remove(pluginsPath / updateEsm));

            std::unordered_set<std::string> activePlugins({
                masterFile,
                blankEsm,
            });
            EXPECT_NO_THROW(loadOrder.setActivePlugins(activePlugins));
        }

        TEST_P(LoadOrderTest, settingActivePluginsShouldDeactivateAnyOthersInLoadOrderCaseInsensitively) {
            std::vector<std::string> validLoadOrder({
                masterFile,
                blankEsm,
                blankEsp,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(validLoadOrder));
            ASSERT_NO_THROW(loadOrder.activate(blankEsp));

            std::unordered_set<std::string> activePlugins({
                masterFile,
                updateEsm,
                boost::to_lower_copy(blankEsm),
            });
            EXPECT_NO_THROW(loadOrder.setActivePlugins(activePlugins));

            std::unordered_set<std::string> expectedActivePlugins({
                masterFile,
                updateEsm,
                blankEsm,
            });
            EXPECT_EQ(expectedActivePlugins, loadOrder.getActivePlugins());
        }

        TEST_P(LoadOrderTest, settingActivePluginsNotInLoadOrderShouldAddThem) {
            std::unordered_set<std::string> activePlugins({
                masterFile,
                updateEsm,
                blankEsm,
            });
            std::vector<std::string> expectedLoadOrder({
                masterFile,
                updateEsm,
                blankEsm,
            });
            ASSERT_TRUE(loadOrder.getLoadOrder().empty());

            EXPECT_NO_THROW(loadOrder.setActivePlugins(activePlugins));

            std::vector<std::string> newLoadOrder(loadOrder.getLoadOrder());
            EXPECT_EQ(3, newLoadOrder.size());
            EXPECT_EQ(1, count(std::begin(newLoadOrder), std::end(newLoadOrder), masterFile));
            EXPECT_EQ(1, count(std::begin(newLoadOrder), std::end(newLoadOrder), updateEsm));
            EXPECT_EQ(1, count(std::begin(newLoadOrder), std::end(newLoadOrder), blankEsm));
        }

        TEST_P(LoadOrderTest, isSynchronisedForTimestampAndAsteriskBasedGames) {
            if (loadOrderMethod == LIBLO_METHOD_TIMESTAMP || loadOrderMethod == LIBLO_METHOD_ASTERISK)
                EXPECT_TRUE(LoadOrder::isSynchronised(gameSettings));
        }

        TEST_P(LoadOrderTest, isSynchronisedForTextfileBasedGamesIfLoadOrderFileDoesNotExist) {
            if (loadOrderMethod != LIBLO_METHOD_TEXTFILE)
                return;

            ASSERT_NO_THROW(boost::filesystem::remove(loadOrderFilePath));

            EXPECT_TRUE(LoadOrder::isSynchronised(gameSettings));
        }

        TEST_P(LoadOrderTest, isSynchronisedForTextfileBasedGamesIfActivePluginsFileDoesNotExist) {
            if (loadOrderMethod != LIBLO_METHOD_TEXTFILE)
                return;

            ASSERT_NO_THROW(boost::filesystem::remove(activePluginsFilePath));

            EXPECT_TRUE(LoadOrder::isSynchronised(gameSettings));
        }

        TEST_P(LoadOrderTest, isSynchronisedForTextfileBasedGamesWhenLoadOrderAndActivePluginsFileContentsAreEquivalent) {
            if (loadOrderMethod != LIBLO_METHOD_TEXTFILE)
                return;

            EXPECT_TRUE(LoadOrder::isSynchronised(gameSettings));
        }

        TEST_P(LoadOrderTest, isNotSynchronisedForTextfileBasedGamesWhenLoadOrderAndActivePluginsFileContentsAreNotEquivalent) {
            if (loadOrderMethod != LIBLO_METHOD_TEXTFILE)
                return;

            boost::filesystem::ofstream out(loadOrderFilePath, std::ios_base::trunc);
            out << blankEsm << std::endl;

            EXPECT_FALSE(LoadOrder::isSynchronised(gameSettings));
        }

        TEST_P(LoadOrderTest, loadingDataShouldNotThrowIfActivePluginsFileDoesNotExist) {
            ASSERT_NO_THROW(boost::filesystem::remove(activePluginsFilePath));

            EXPECT_NO_THROW(loadOrder.load());
        }

        TEST_P(LoadOrderTest, loadingDataShouldActivateNoPluginsIfActivePluginsFileDoesNotExist) {
            ASSERT_NO_THROW(boost::filesystem::remove(activePluginsFilePath));

            ASSERT_NO_THROW(loadOrder.load());

            EXPECT_TRUE(loadOrder.getActivePlugins().empty());
        }

        TEST_P(LoadOrderTest, loadingDataShouldActivateTheGameMasterForTextfileAndAsteriskBasedGamesAndNotOtherwise) {
            EXPECT_NO_THROW(loadOrder.load());

            int count = loadOrder.getActivePlugins().count(masterFile);
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK)
                EXPECT_EQ(1, count);
            else
                EXPECT_EQ(0, count);
        }

        TEST_P(LoadOrderTest, loadingDataShouldActivateUpdateEsmWhenItExistsForSkyrimAndNotOtherwise) {
            EXPECT_NO_THROW(loadOrder.load());

            int count = loadOrder.getActivePlugins().count(updateEsm);
            if (GetParam() == LIBLO_GAME_TES5)
                EXPECT_EQ(1, count);
            else
                EXPECT_EQ(0, count);
        }

        TEST_P(LoadOrderTest, loadingDataShouldNotActivateUpdateEsmWhenItDoesNotExist) {
            ASSERT_NO_THROW(boost::filesystem::remove(pluginsPath / updateEsm));

            EXPECT_NO_THROW(loadOrder.load());

            EXPECT_EQ(0, loadOrder.getActivePlugins().count(updateEsm));
        }

        TEST_P(LoadOrderTest, loadingDataWithMoreThanMaxNumberActivePluginsShouldStopWhenMaxIsReached) {
            // Create plugins to test active plugins limit with. Do it
            // here because it's too expensive to do for every test.
            std::unordered_set<std::string> expectedActivePlugins;

            std::string linePrefix = getActivePluginsFileLinePrefix();
            boost::filesystem::ofstream out(activePluginsFilePath);

            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK) {
                out << linePrefix << utf8ToWindows1252(masterFile) << std::endl;
                expectedActivePlugins.insert(masterFile);

                if (GetParam() == LIBLO_GAME_TES5) {
                    out << linePrefix << utf8ToWindows1252(updateEsm) << std::endl;
                    expectedActivePlugins.insert(updateEsm);
                }
            }

            for (size_t i = 0; i < LoadOrder::maxActivePlugins - expectedActivePlugins.size(); ++i) {
                std::string filename = std::to_string(i) + ".esp";
                EXPECT_NO_THROW(boost::filesystem::copy_file(pluginsPath / blankEsp, pluginsPath / filename));
                out << linePrefix << filename << std::endl;
                expectedActivePlugins.insert(filename);
            }
            out.close();

            EXPECT_NO_THROW(loadOrder.load());

            EXPECT_EQ(expectedActivePlugins.size(), loadOrder.getActivePlugins().size());
            EXPECT_EQ(expectedActivePlugins, loadOrder.getActivePlugins());

            for (size_t i = 0; i < LoadOrder::maxActivePlugins; ++i)
                EXPECT_NO_THROW(boost::filesystem::remove(pluginsPath / (std::to_string(i) + ".esp")));
        }

        TEST_P(LoadOrderTest, loadingDataShouldFixInvalidDataWhenReadingActivePluginsFile) {
            EXPECT_NO_THROW(loadOrder.load());

            std::unordered_set<std::string> expectedActivePlugins({
                nonAsciiEsm,
                blankEsm,
                blankEsp,
            });
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK) {
                expectedActivePlugins.insert(masterFile);

                if (GetParam() == LIBLO_GAME_TES5)
                    expectedActivePlugins.insert(updateEsm);
            }
            EXPECT_EQ(expectedActivePlugins, loadOrder.getActivePlugins());
        }

        TEST_P(LoadOrderTest, loadingDataShouldPreferLoadOrderFileForTextfileBasedGamesOtherwiseUseTimestamps) {
            EXPECT_NO_THROW(loadOrder.load());

            std::vector<std::string> expectedLoadOrder;
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE) {
                expectedLoadOrder = std::vector<std::string>({
                    masterFile,
                    nonAsciiEsm,
                    blankDifferentEsm,
                    blankEsm,
                    blankMasterDependentEsm,
                    blankDifferentMasterDependentEsm,
                    updateEsm,
                });
                EXPECT_TRUE(equal(begin(expectedLoadOrder), end(expectedLoadOrder), begin(loadOrder.getLoadOrder())));
            }
            else {
                expectedLoadOrder = std::vector<std::string>({
                    nonAsciiEsm,
                    masterFile,
                    blankDifferentEsm,
                    blankEsm,
                    blankMasterDependentEsm,
                    blankDifferentMasterDependentEsm,
                    updateEsm,
                    blankEsp,
                    blankDifferentEsp,
                    blankMasterDependentEsp,
                    blankDifferentMasterDependentEsp,
                    blankPluginDependentEsp,
                    blankDifferentPluginDependentEsp,
                });

                // Asterisk-based games load their master file first.
                if (loadOrderMethod == LIBLO_METHOD_ASTERISK) {
                    expectedLoadOrder.erase(++begin(expectedLoadOrder));
                    expectedLoadOrder.insert(begin(expectedLoadOrder), masterFile);
                }

                EXPECT_EQ(expectedLoadOrder, loadOrder.getLoadOrder());
            }
        }

        TEST_P(LoadOrderTest, loadingDataShouldFallBackToActivePluginsFileForTextfileBasedGames) {
            if (loadOrderMethod != LIBLO_METHOD_TEXTFILE)
                return;

            ASSERT_NO_THROW(boost::filesystem::remove(loadOrderFilePath));

            EXPECT_NO_THROW(loadOrder.load());

            std::vector<std::string> expectedLoadOrder;
            expectedLoadOrder = std::vector<std::string>({
                masterFile,
                nonAsciiEsm,
                blankEsm,
            });
            if (GetParam() == LIBLO_GAME_TES5)
                expectedLoadOrder.push_back(updateEsm);

            EXPECT_TRUE(equal(begin(expectedLoadOrder), end(expectedLoadOrder), begin(loadOrder.getLoadOrder())));
        }

        TEST_P(LoadOrderTest, loadingDataTwiceShouldReloadTheActivePluginsIfTheyHaveBeenChanged) {
            ASSERT_NO_THROW(loadOrder.load());

            writeLoadOrder({{blankEsp, true}});
            incrementModTime(activePluginsFilePath);

            EXPECT_NO_THROW(loadOrder.load());

            std::unordered_set<std::string> expectedActivePlugins({
                blankEsp,
            });
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK) {
                expectedActivePlugins.insert(masterFile);

                if (GetParam() == LIBLO_GAME_TES5)
                    expectedActivePlugins.insert(updateEsm);
            }

            EXPECT_EQ(expectedActivePlugins, loadOrder.getActivePlugins());
        }

        TEST_P(LoadOrderTest, loadingDataTwiceShouldReloadTheActivePluginsIfTheyHaveBeenChangedAndFileHasOlderTimestamp) {
            ASSERT_NO_THROW(loadOrder.load());

            writeLoadOrder({{blankEsp, true}});
            decrementModTime(activePluginsFilePath);

            EXPECT_NO_THROW(loadOrder.load());

            std::unordered_set<std::string> expectedActivePlugins({
                blankEsp,
            });
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE || loadOrderMethod == LIBLO_METHOD_ASTERISK) {
                expectedActivePlugins.insert(masterFile);

                if (GetParam() == LIBLO_GAME_TES5)
                    expectedActivePlugins.insert(updateEsm);
            }

            EXPECT_EQ(expectedActivePlugins, loadOrder.getActivePlugins());
        }

        TEST_P(LoadOrderTest, loadingDataTwiceShouldReloadTheLoadOrderIfItHasBeenChangedForTextfileBasedGames) {
            if (loadOrderMethod != LIBLO_METHOD_TEXTFILE)
                return;

            ASSERT_NO_THROW(loadOrder.load());

            writeLoadOrder({{blankDifferentEsm, false}});
            incrementModTime(loadOrderFilePath);

            EXPECT_NO_THROW(loadOrder.load());

            std::vector<std::string> expectedLoadOrder({
                nonAsciiEsm,
                masterFile,
                blankDifferentEsm,
                blankEsm,
                blankMasterDependentEsm,
                blankDifferentMasterDependentEsm,
                updateEsm,
                blankEsp,
                blankDifferentEsp,
                blankMasterDependentEsp,
                blankDifferentMasterDependentEsp,
                blankPluginDependentEsp,
                blankDifferentPluginDependentEsp,
            });
            if (loadOrderMethod == LIBLO_METHOD_TEXTFILE) {
                EXPECT_NE(expectedLoadOrder, loadOrder.getLoadOrder());
                EXPECT_TRUE(is_permutation(begin(expectedLoadOrder), end(expectedLoadOrder), begin(loadOrder.getLoadOrder())));
            }
            else
                EXPECT_EQ(expectedLoadOrder, loadOrder.getLoadOrder());
        }

        TEST_P(LoadOrderTest, loadingDataTwiceShouldReloadTheLoadOrderIfItHasBeenChangedForTextfileBasedGamesAndFileHasOlderTimestamp) {
            if (loadOrderMethod != LIBLO_METHOD_TEXTFILE)
                return;

            ASSERT_NO_THROW(loadOrder.load());

            writeLoadOrder({{blankDifferentEsm, false}});
            decrementModTime(loadOrderFilePath);

            EXPECT_NO_THROW(loadOrder.load());

            std::vector<std::string> expectedLoadOrder({
                nonAsciiEsm,
                masterFile,
                blankDifferentEsm,
                blankEsm,
                blankMasterDependentEsm,
                blankDifferentMasterDependentEsm,
                updateEsm,
                blankEsp,
                blankDifferentEsp,
                blankMasterDependentEsp,
                blankDifferentMasterDependentEsp,
                blankPluginDependentEsp,
                blankDifferentPluginDependentEsp,
            });
            EXPECT_NE(expectedLoadOrder, loadOrder.getLoadOrder());
            EXPECT_TRUE(is_permutation(begin(expectedLoadOrder), end(expectedLoadOrder), begin(loadOrder.getLoadOrder())));
        }

        TEST_P(LoadOrderTest, loadingDataTwiceShouldReloadFromThePluginsFolderIfItHasBeenChanged) {
            ASSERT_NO_THROW(loadOrder.load());

            ASSERT_NO_THROW(boost::filesystem::remove(pluginsPath / nonAsciiEsm));
            incrementModTime(pluginsPath);

            EXPECT_NO_THROW(loadOrder.load());

            std::vector<std::string> expectedLoadOrder({
                masterFile,
                blankDifferentEsm,
                blankEsm,
                blankMasterDependentEsm,
                blankDifferentMasterDependentEsm,
                updateEsm,
                blankEsp,
                blankDifferentEsp,
                blankMasterDependentEsp,
                blankDifferentMasterDependentEsp,
                blankPluginDependentEsp,
                blankDifferentPluginDependentEsp,
            });
            EXPECT_EQ(expectedLoadOrder, loadOrder.getLoadOrder());
        }

        TEST_P(LoadOrderTest, loadingDataTwiceShouldReloadFromThePluginsFolderIfItHasBeenChangedAndFolderHasOlderTimestamp) {
            ASSERT_NO_THROW(loadOrder.load());

            ASSERT_NO_THROW(boost::filesystem::remove(pluginsPath / nonAsciiEsm));
            decrementModTime(pluginsPath);

            EXPECT_NO_THROW(loadOrder.load());

            std::vector<std::string> expectedLoadOrder({
                masterFile,
                blankDifferentEsm,
                blankEsm,
                blankMasterDependentEsm,
                blankDifferentMasterDependentEsm,
                updateEsm,
                blankEsp,
                blankDifferentEsp,
                blankMasterDependentEsp,
                blankDifferentMasterDependentEsp,
                blankPluginDependentEsp,
                blankDifferentPluginDependentEsp,
            });
            EXPECT_EQ(expectedLoadOrder, loadOrder.getLoadOrder());
        }

        TEST_P(LoadOrderTest, loadingDataTwiceShouldReloadAPluginIfItHasBeenEdited) {
            ASSERT_NO_THROW(loadOrder.load());

            boost::filesystem::ofstream out(pluginsPath / updateEsm);
            out << std::endl;
            out.close();
            incrementModTime(pluginsPath / updateEsm);

            EXPECT_NO_THROW(loadOrder.load());

            EXPECT_EQ(loadOrder.getLoadOrder().size(), loadOrder.getPosition(updateEsm));
        }

        TEST_P(LoadOrderTest, loadingDataTwiceShouldReloadAPluginIfItHasBeenEditedAndFileHasOlderTimestamp) {
            ASSERT_NO_THROW(loadOrder.load());

            boost::filesystem::ofstream out(pluginsPath / updateEsm);
            out << std::endl;
            out.close();
            decrementModTime(pluginsPath / updateEsm);

            EXPECT_NO_THROW(loadOrder.load());

            EXPECT_EQ(loadOrder.getLoadOrder().size(), loadOrder.getPosition(updateEsm));
        }

        TEST_P(LoadOrderTest, savingShouldSetTimestampsForTimestampBasedGamesAndWriteToLoadOrderAndActivePluginsFilesOtherwise) {
            std::vector<std::string> plugins({
                masterFile,
                blankEsm,
                blankMasterDependentEsm,
                blankDifferentEsm,
                blankDifferentMasterDependentEsm,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(plugins));

            EXPECT_NO_THROW(loadOrder.save());

            ASSERT_NO_THROW(loadOrder.load());

            EXPECT_TRUE(equal(begin(plugins), end(plugins), begin(loadOrder.getLoadOrder())));
        }

        TEST_P(LoadOrderTest, savingShouldWriteActivePluginsToActivePluginsFile) {
            std::unordered_set<std::string> activePlugins({
                masterFile,
                updateEsm,
                blankEsm,
            });
            ASSERT_NO_THROW(loadOrder.setActivePlugins(activePlugins));

            EXPECT_NO_THROW(loadOrder.save());

            ASSERT_NO_THROW(loadOrder.load());

            EXPECT_EQ(activePlugins, loadOrder.getActivePlugins());
        }

        TEST_P(LoadOrderTest, savingShouldWriteWholeLoadOrderToActivePluginsFileWithAsteriskPrefixesForActivePluginsForAsteriskBasedGames) {
            if (loadOrderMethod != LIBLO_METHOD_ASTERISK)
                return;

            std::vector<std::string> plugins({
                masterFile,
                blankEsm,
                blankMasterDependentEsm,
                blankDifferentEsm,
                nonAsciiEsm,
                blankDifferentMasterDependentEsm,
                updateEsm,
                blankMasterDependentEsp,
                blankDifferentEsp,
                blankDifferentPluginDependentEsp,
                blankEsp,
                blankDifferentMasterDependentEsp,
                blankPluginDependentEsp,
            });
            std::unordered_set<std::string> activePlugins({
                masterFile,
                blankEsm,
                blankDifferentEsp,
            });
            ASSERT_NO_THROW(loadOrder.setLoadOrder(plugins));
            ASSERT_NO_THROW(loadOrder.setActivePlugins(activePlugins));
            EXPECT_NO_THROW(loadOrder.save());

            boost::filesystem::ifstream in(activePluginsFilePath);
            std::vector<std::string> lines;
            while (in) {
                std::string line;
                std::getline(in, line);

                if (!line.empty())
                    lines.push_back(windows1252toUtf8(line));
            }

            std::vector<std::string> expectedLines({
                '*' + blankEsm,
                blankMasterDependentEsm,
                blankDifferentEsm,
                nonAsciiEsm,
                blankDifferentMasterDependentEsm,
                updateEsm,
                blankMasterDependentEsp,
                '*' + blankDifferentEsp,
                blankDifferentPluginDependentEsp,
                blankEsp,
                blankDifferentMasterDependentEsp,
                blankPluginDependentEsp,
            });

            EXPECT_EQ(expectedLines, lines);
        }
    }
}
