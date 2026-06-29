cmake_minimum_required(VERSION 3.16)
project(medius_cpp LANGUAGES CXX)

option(MEDIUS_BUILD_EXAMPLES "Build the C++ examples" OFF)
option(MEDIUS_BUILD_TESTS "Build the C++ tests" OFF)
option(MEDIUS_FEATURE_MOCK "Compile and link against the mock surface (MockBox)" OFF)
option(MEDIUS_FEATURE_FLASH "Compile and link against the flash surface" OFF)
option(MEDIUS_CARGO_BUILD "Build medius-capi via cargo at configure time" OFF)
option(MEDIUS_SANITIZE "Build the tests with ASan + UBSan" OFF)

set(MEDIUS_WORKSPACE_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../.."
    CACHE PATH "medius cargo workspace root")
set(MEDIUS_CAPI_INCLUDE_DIR "${MEDIUS_WORKSPACE_ROOT}/medius-capi/include"
    CACHE PATH "directory holding medius.h")
set(MEDIUS_PROFILE "debug" CACHE STRING "cargo profile dir to search (debug|release)")
set(MEDIUS_CAPI_LIB "" CACHE FILEPATH "path to libmedius_capi (auto-detected if empty)")

if(MEDIUS_CARGO_BUILD)
    set(_features "")
    if(MEDIUS_FEATURE_MOCK)
        list(APPEND _features mock)
    endif()
    if(MEDIUS_FEATURE_FLASH)
        list(APPEND _features flash)
    endif()
    set(_feature_args "")
    if(_features)
        string(REPLACE ";" "," _feat "${_features}")
        set(_feature_args --features ${_feat})
    endif()
    set(_profile_args "")
    if(MEDIUS_PROFILE STREQUAL "release")
        set(_profile_args --release)
    endif()
    message(STATUS "medius: building medius-capi via cargo (${MEDIUS_PROFILE})")
    execute_process(
        COMMAND cargo build -p medius-capi ${_profile_args} ${_feature_args}
        WORKING_DIRECTORY "${MEDIUS_WORKSPACE_ROOT}"
        RESULT_VARIABLE _cargo_rc)
    if(NOT _cargo_rc EQUAL 0)
        message(FATAL_ERROR "cargo build of medius-capi failed (${_cargo_rc})")
    endif()
endif()

if(MEDIUS_CAPI_LIB)
    set(_medius_lib "${MEDIUS_CAPI_LIB}")
else()
    find_library(MEDIUS_CAPI_LIB_FOUND
        NAMES medius_capi
        PATHS "${MEDIUS_WORKSPACE_ROOT}/target/${MEDIUS_PROFILE}"
        NO_DEFAULT_PATH)
    set(_medius_lib "${MEDIUS_CAPI_LIB_FOUND}")
endif()
if(NOT _medius_lib)
    message(FATAL_ERROR
        "libmedius_capi not found. Build it (cargo build -p medius-capi [--features mock]) "
        "or set -DMEDIUS_CAPI_LIB=/path/to/libmedius_capi.so")
endif()
get_filename_component(MEDIUS_CAPI_LIBDIR "${_medius_lib}" DIRECTORY)
message(STATUS "medius: linking ${_medius_lib}")

add_library(medius_capi_imported UNKNOWN IMPORTED GLOBAL)
set_target_properties(medius_capi_imported PROPERTIES
    IMPORTED_LOCATION "${_medius_lib}")

add_library(medius INTERFACE)
add_library(medius::medius ALIAS medius)
target_compile_features(medius INTERFACE cxx_std_17)
target_include_directories(medius INTERFACE
    "${CMAKE_CURRENT_SOURCE_DIR}/include"
    "${MEDIUS_CAPI_INCLUDE_DIR}")
target_link_libraries(medius INTERFACE medius_capi_imported)
if(MEDIUS_FEATURE_MOCK)
    target_compile_definitions(medius INTERFACE MEDIUS_FEATURE_MOCK)
endif()
if(MEDIUS_FEATURE_FLASH)
    target_compile_definitions(medius INTERFACE MEDIUS_FEATURE_FLASH)
endif()

if(MEDIUS_BUILD_EXAMPLES)
    add_executable(quickstart examples/quickstart.cpp)
    target_link_libraries(quickstart PRIVATE medius::medius)
    target_compile_options(quickstart PRIVATE -Wall -Wextra)
    set_target_properties(quickstart PROPERTIES BUILD_RPATH "${MEDIUS_CAPI_LIBDIR}")
endif()

if(MEDIUS_BUILD_TESTS)
    enable_testing()
    add_executable(medius_mock_test tests/test_mock.cpp tests/test_odr.cpp)
    target_link_libraries(medius_mock_test PRIVATE medius::medius)
    target_compile_options(medius_mock_test PRIVATE -Wall -Wextra)
    set_target_properties(medius_mock_test PROPERTIES BUILD_RPATH "${MEDIUS_CAPI_LIBDIR}")
    if(MEDIUS_SANITIZE)
        target_compile_options(medius_mock_test PRIVATE
            -fsanitize=address,undefined -fno-omit-frame-pointer -g)
        target_link_options(medius_mock_test PRIVATE -fsanitize=address,undefined)
    endif()
    add_test(NAME medius_mock_test COMMAND medius_mock_test)
endif()
