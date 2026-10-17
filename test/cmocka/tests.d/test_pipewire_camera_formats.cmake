# Real SPA format enumeration without a camera, portal, daemon or display (#1068).
if(OS_LINUX AND TARGET OBS::pipewire)
  find_package(PipeWire 0.3.60 QUIET)
endif()
if(OS_LINUX AND TARGET OBS::pipewire AND PipeWire_FOUND)
  find_package(PkgConfig REQUIRED)
  pkg_check_modules(_camera_formats_test_glib REQUIRED IMPORTED_TARGET glib-2.0 gobject-2.0)
  get_target_property(_camera_formats_drm_includes Libdrm::Libdrm INTERFACE_INCLUDE_DIRECTORIES)
  add_executable(
    test_pipewire_camera_formats
    test_pipewire_camera_formats.c
    "${CMAKE_SOURCE_DIR}/plugins/linux-pipewire/formats.c.h"
    "${CMAKE_SOURCE_DIR}/plugins/linux-pipewire/frame-layout.c.h"
    "${CMAKE_SOURCE_DIR}/plugins/linux-pipewire/frame-pacing.c.h"
    "${CMAKE_SOURCE_DIR}/plugins/linux-pipewire/pipewire.c.h"
    "${CMAKE_SOURCE_DIR}/plugins/linux-pipewire/portal.c.h"
    "${CMAKE_SOURCE_DIR}/plugins/linux-pipewire/reconnect-policy.c.h"
  )
  target_include_directories(
    test_pipewire_camera_formats
    PRIVATE ${CMOCKA_INCLUDE_DIR} "${CMAKE_SOURCE_DIR}/plugins/linux-pipewire" ${_camera_formats_drm_includes}
  )
  target_link_libraries(
    test_pipewire_camera_formats
    PRIVATE OBS::libobs OBS::glad PipeWire::PipeWire gio::gio PkgConfig::_camera_formats_test_glib ${CMOCKA_LIBRARIES}
  )
  add_dependencies(cmocka-tests test_pipewire_camera_formats)
  add_test(NAME test_pipewire_camera_formats COMMAND test_pipewire_camera_formats)
  set_tests_properties(test_pipewire_camera_formats PROPERTIES TIMEOUT 10)
endif()
