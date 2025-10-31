cmake_minimum_required(VERSION 3.15)

project(JUCEBenchmarks VERSION 1.0.0)

# Add benchmark subdirectories
add_subdirectory(simple-sine)
add_subdirectory(medium-graph)
add_subdirectory(complex-graph)

# Future benchmarks can be added here:
# add_subdirectory(polysynth)
