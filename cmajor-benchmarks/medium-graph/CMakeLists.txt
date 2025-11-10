cmake_minimum_required(VERSION 3.15)

project(MediumGraphCmajorBenchmark VERSION 1.0.0)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Fetch Cmajor
include(FetchContent)
FetchContent_Declare(
    cmajor
    GIT_REPOSITORY https://github.com/SoundStacks/cmajor.git
    GIT_TAG        main
    GIT_SHALLOW    TRUE
)
FetchContent_MakeAvailable(cmajor)

# Create the benchmark executable
add_executable(medium-graph main.cpp)

target_include_directories(medium-graph
    PRIVATE
        ${cmajor_SOURCE_DIR}/include)

target_link_libraries(medium-graph
    PRIVATE
        CmajorEngine
        pthread
        dl)

target_compile_options(medium-graph
    PRIVATE
        -O3
        -march=native)

# Copy the .cmajor file to build directory
configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/MediumGraph.cmajor
    ${CMAKE_CURRENT_BINARY_DIR}/MediumGraph.cmajor
    COPYONLY)
