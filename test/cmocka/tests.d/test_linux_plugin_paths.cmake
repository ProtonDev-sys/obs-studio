# Exercise the actual Linux loader with a recording libobs boundary. This
# needs neither Qt nor a running libobs core (obs-rust/obs-studio#1084).
if(OS_LINUX)
  block()
    set(_loader_fixture "${CMAKE_CURRENT_BINARY_DIR}/linux-plugin-paths")
    set(OBS_PLATFORM_INSTALL_PATH "/usr/")
    set(OBS_PLATFORM_LIBRARY_PATH "lib/")
    set(OBS_PLATFORM_DATA_PATH "share/")
    set(OBS_PLATFORM_PLUGIN_MODULE_PATH "/usr/lib/obs-modules/plugins")
    set(OBS_PLATFORM_PLUGIN_DATA_PATH "/usr/share/obs/obs-modules/plugins")
    configure_file(
      "${CMAKE_SOURCE_DIR}/frontend/plugin-manager/LoaderPaths_Linux.hpp.in"
      "${_loader_fixture}/LoaderPaths_Linux.hpp"
      @ONLY
    )
    add_executable(
      test_linux_plugin_paths
      test_linux_plugin_paths.cpp
      "${CMAKE_SOURCE_DIR}/frontend/plugin-manager/PluginModuleLoader_Linux.cpp"
    )
    target_compile_features(test_linux_plugin_paths PRIVATE cxx_std_17)
    target_include_directories(
      test_linux_plugin_paths
      PRIVATE
        ${CMOCKA_INCLUDE_DIR}
        "${_loader_fixture}"
        "${CMAKE_SOURCE_DIR}/frontend/plugin-manager"
        "$<TARGET_PROPERTY:OBS::libobs,INTERFACE_INCLUDE_DIRECTORIES>"
    )
    target_link_libraries(test_linux_plugin_paths PRIVATE ${CMOCKA_LIBRARIES})
    add_dependencies(cmocka-tests test_linux_plugin_paths)
    add_test(test_linux_plugin_paths ${CMAKE_CURRENT_BINARY_DIR}/test_linux_plugin_paths)
  endblock()
endif()
