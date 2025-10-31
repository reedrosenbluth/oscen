cmake_minimum_required(VERSION 3.15)

project(MediumGraphBenchmark VERSION 1.0.0)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Fetch JUCE if not already available
include(FetchContent)
FetchContent_Declare(
    JUCE
    GIT_REPOSITORY https://github.com/juce-framework/JUCE.git
    GIT_TAG        8.0.0
    GIT_SHALLOW    TRUE
)
FetchContent_MakeAvailable(JUCE)

# Create the benchmark executable
juce_add_console_app(medium-graph PRODUCT_NAME "medium-graph")

target_sources(medium-graph
    PRIVATE
        main.cpp)

target_compile_definitions(medium-graph
    PRIVATE
        JUCE_WEB_BROWSER=0
        JUCE_USE_CURL=0
        JUCE_VST3_CAN_REPLACE_VST2=0
        JUCE_HEADLESS_PLUGIN_CLIENT=1
        JUCE_PLUGINHOST_VST=0
        JUCE_PLUGINHOST_VST3=0
        JUCE_PLUGINHOST_AU=0
        JUCE_PLUGINHOST_LADSPA=0)

target_link_libraries(medium-graph
    PRIVATE
        juce::juce_core
        juce::juce_audio_basics
        juce::juce_audio_processors
        juce::juce_dsp
    PUBLIC
        juce::juce_recommended_config_flags
        juce::juce_recommended_lto_flags
        juce::juce_recommended_warning_flags)
