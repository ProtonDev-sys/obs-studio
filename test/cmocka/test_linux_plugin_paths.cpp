/* Linux plugin path selection (obs-rust/obs-studio#1084).
 *
 * The manylinux AppImage scanned /usr/lib/obs-plugins and tried to load the
 * host distribution's incompatible plugins. Compile the actual Linux loader
 * against a recording obs_load_plugins boundary: AppImage launches must keep
 * explicit and per-user paths, but omit both compiled system plugin paths.
 * Native launches and portable mode retain their existing path selection.
 * No Qt frontend, libobs core, display, or installed plugins are needed.
 */

#include <PluginManager.hpp>

#include <stdarg.h>
#include <stddef.h>
#include <setjmp.h>
#include <stdint.h>
extern "C" {
#include <cmocka.h>
}

#include <array>
#include <cstdlib>
#include <initializer_list>
#include <optional>
#include <string>
#include <vector>

struct PluginPath {
	std::string binary;
	std::string data;
	obs_runtime_module_type type;
};

static std::vector<PluginPath> loaded_paths;
static constexpr std::array environment_variables{
	"OBS_APPIMAGE",
	"APPDIR",
	"OBS_PLUGINS_PATH",
	"OBS_LEGACY_PLUGINS_PATH",
	"OBS_LEGACY_PLUGINS_DATA_PATH",
	"XDG_CONFIG_HOME",
	"XDG_DATA_HOME",
	"HOME",
};
static std::array<std::optional<std::string>, environment_variables.size()> saved_environment;

extern "C" void obs_load_plugins(obs_runtime_module_info *info, obs_module_failure_info *failure)
{
	loaded_paths.push_back({info->path_info.binary, info->path_info.data, info->type});
	*failure = {};
}

extern "C" void obs_module_failure_info_free(obs_module_failure_info *) {}

/* The UI-coupled loadAllPlugins wrapper is a trampoline to the two private
 * methods compiled from PluginModuleLoader_Linux.cpp. It does not emulate any
 * path decision. The state operators only combine the successful stub calls.
 */
void OBS::PluginManager::loadAllPlugins(bool usePortableMode)
{
	assert_int_equal(static_cast<int>(loadPlugins(usePortableMode)), static_cast<int>(State::Success));
	assert_int_equal(static_cast<int>(loadLegacyPlugins(usePortableMode)), static_cast<int>(State::Success));
}

bool operator&&(const OBS::PluginManager::State &lhs, const OBS::PluginManager::State &rhs)
{
	return lhs == OBS::PluginManager::State::Success && rhs == OBS::PluginManager::State::Success;
}

bool operator&&(bool lhs, const OBS::PluginManager::State &rhs)
{
	return lhs && rhs == OBS::PluginManager::State::Success;
}

static int setup(void **)
{
	for (size_t i = 0; i < environment_variables.size(); ++i) {
		const char *value = getenv(environment_variables[i]);
		saved_environment[i] = value ? std::optional<std::string>{value} : std::nullopt;
		assert_int_equal(unsetenv(environment_variables[i]), 0);
	}
	assert_int_equal(setenv("HOME", "/home/obs-plugin-test", 1), 0);
	loaded_paths.clear();
	return 0;
}

static int teardown(void **)
{
	for (size_t i = 0; i < environment_variables.size(); ++i) {
		if (saved_environment[i]) {
			assert_int_equal(setenv(environment_variables[i], saved_environment[i]->c_str(), 1), 0);
		} else {
			assert_int_equal(unsetenv(environment_variables[i]), 0);
		}
	}
	return 0;
}

static const PluginPath system_modern{"/usr/lib/obs-modules/plugins", "/usr/share/obs/obs-modules/plugins/%module%/",
				      OBS_MODULE_TYPE_PLUGIN};
static const PluginPath home_modern{"/home/obs-plugin-test/.local/share/obs-studio/plugins/%module%/",
				    "/home/obs-plugin-test/.local/share/obs-studio/plugins/%module%/data/",
				    OBS_MODULE_TYPE_PLUGIN};
static const PluginPath system_legacy{"/usr/lib/obs-plugins/", "/usr/share/obs/obs-plugins/%module%",
				      OBS_MODULE_TYPE_LEGACY_PLUGIN};
static const PluginPath home_legacy{"/home/obs-plugin-test/.config/obs-studio/plugins/%module%/bin/64bit",
				    "/home/obs-plugin-test/.config/obs-studio/plugins/%module%/data",
				    OBS_MODULE_TYPE_LEGACY_PLUGIN};
static const PluginPath explicit_modern{"/selected/plugins/%module%/", "/selected/plugins/%module%/data/",
					OBS_MODULE_TYPE_PLUGIN};
static const PluginPath explicit_legacy{"/selected/legacy", "/selected/legacy-data/%module%/",
					OBS_MODULE_TYPE_LEGACY_PLUGIN};
static const PluginPath xdg_modern{"/selected/data/obs-studio/plugins/%module%/",
				   "/selected/data/obs-studio/plugins/%module%/data/", OBS_MODULE_TYPE_PLUGIN};
static const PluginPath xdg_legacy{"/selected/config/obs-studio/plugins/%module%/bin/64bit",
				   "/selected/config/obs-studio/plugins/%module%/data", OBS_MODULE_TYPE_LEGACY_PLUGIN};

static void check_paths(bool portable, std::initializer_list<PluginPath> expected)
{
	OBS::PluginManager manager;
	manager.loadAllPlugins(portable);
	assert_int_equal(loaded_paths.size(), expected.size());
	size_t i = 0;
	for (const PluginPath &path : expected) {
		assert_string_equal(loaded_paths[i].binary.c_str(), path.binary.c_str());
		assert_string_equal(loaded_paths[i].data.c_str(), path.data.c_str());
		assert_int_equal(loaded_paths[i].type, path.type);
		++i;
	}
}

static void select_user_paths()
{
	assert_int_equal(setenv("OBS_PLUGINS_PATH", "/selected/plugins", 1), 0);
	assert_int_equal(setenv("OBS_LEGACY_PLUGINS_PATH", "/selected/legacy", 1), 0);
	assert_int_equal(setenv("OBS_LEGACY_PLUGINS_DATA_PATH", "/selected/legacy-data", 1), 0);
	assert_int_equal(setenv("XDG_DATA_HOME", "/selected/data", 1), 0);
	assert_int_equal(setenv("XDG_CONFIG_HOME", "/selected/config", 1), 0);
}

static void test_native_paths(void **)
{
	check_paths(false, {system_modern, home_modern, system_legacy, home_legacy});
}

static void test_appimage_paths(void **)
{
	assert_int_equal(setenv("OBS_APPIMAGE", "1", 1), 0);
	check_paths(false, {home_modern, home_legacy});
}

static void test_appimage_explicit_and_xdg_paths(void **)
{
	assert_int_equal(setenv("OBS_APPIMAGE", "1", 1), 0);
	select_user_paths();
	check_paths(false, {explicit_modern, xdg_modern, explicit_legacy, xdg_legacy});
}

static void test_appimage_portable_paths(void **)
{
	assert_int_equal(setenv("OBS_APPIMAGE", "1", 1), 0);
	select_user_paths();
	check_paths(true, {explicit_modern, explicit_legacy});
}

static void test_native_portable_paths(void **)
{
	select_user_paths();
	check_paths(true, {explicit_modern, system_modern, explicit_legacy, system_legacy});
}

static void test_inherited_appdir_keeps_native_paths(void **)
{
	assert_int_equal(setenv("APPDIR", "/another/application.AppDir", 1), 0);
	check_paths(false, {system_modern, home_modern, system_legacy, home_legacy});
}

static void test_disabled_marker_keeps_native_paths(void **)
{
	assert_int_equal(setenv("OBS_APPIMAGE", "0", 1), 0);
	check_paths(false, {system_modern, home_modern, system_legacy, home_legacy});
}

int main()
{
	const CMUnitTest tests[] = {
		cmocka_unit_test_setup_teardown(test_native_paths, setup, teardown),
		cmocka_unit_test_setup_teardown(test_appimage_paths, setup, teardown),
		cmocka_unit_test_setup_teardown(test_appimage_explicit_and_xdg_paths, setup, teardown),
		cmocka_unit_test_setup_teardown(test_appimage_portable_paths, setup, teardown),
		cmocka_unit_test_setup_teardown(test_native_portable_paths, setup, teardown),
		cmocka_unit_test_setup_teardown(test_inherited_appdir_keeps_native_paths, setup, teardown),
		cmocka_unit_test_setup_teardown(test_disabled_marker_keeps_native_paths, setup, teardown),
	};
	return cmocka_run_group_tests(tests, nullptr, nullptr);
}
