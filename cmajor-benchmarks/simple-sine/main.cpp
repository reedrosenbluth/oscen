/*
  Cmajor Simple Sine Benchmark

  Benchmarks a single sine oscillator to compare with oscen and JUCE.

  Run with: ./simple-sine
*/

#include "cmajor/API/cmaj_Engine.h"
#include <chrono>
#include <iostream>
#include <iomanip>
#include <fstream>
#include <sstream>

std::string loadFile(const std::string& path)
{
    std::ifstream file(path);
    if (!file.is_open())
    {
        std::cerr << "Failed to open file: " << path << std::endl;
        exit(1);
    }
    std::stringstream buffer;
    buffer << file.rdbuf();
    return buffer.str();
}

void runBenchmark()
{
    const int numSamples = 441000;  // 10 seconds at 44.1kHz
    const double sampleRate = 44100.0;
    const int blockSize = 512;

    // Create Cmajor engine
    auto engine = cmaj::Engine::create();

    if (!engine)
    {
        std::cerr << "Failed to create Cmajor engine" << std::endl;
        return;
    }

    // Load the Cmajor program
    std::string cmajorCode = loadFile("SimpleSine.cmajor");

    cmaj::ProgramInterface program;
    program.name = "SimpleSine";
    program.mainProcessor = "SimpleSine";

    auto buildSettings = engine->createBuildSettings();
    buildSettings->setFrequency(sampleRate);
    buildSettings->setBlockSize(blockSize);

    // Compile the program
    auto compileResult = engine->load(program, cmajorCode.c_str(), buildSettings);

    if (!compileResult.isOK())
    {
        std::cerr << "Failed to compile Cmajor program: "
                  << compileResult.getErrorMessage() << std::endl;
        return;
    }

    // Link and prepare for execution
    if (!engine->link().isOK())
    {
        std::cerr << "Failed to link program" << std::endl;
        return;
    }

    // Create audio buffers
    std::vector<float> outputBuffer(blockSize);

    std::cout << "=== Cmajor Simple Sine (1 oscillator) ===" << std::endl;
    std::cout << "Processing " << numSamples << " samples..." << std::endl;

    auto start = std::chrono::high_resolution_clock::now();

    int samplesProcessed = 0;
    while (samplesProcessed < numSamples)
    {
        int samplesToProcess = std::min(blockSize, numSamples - samplesProcessed);

        // Process the block
        engine->advance();

        // Get output (this would normally copy from engine's output)
        // For benchmarking, we just need to advance the engine

        samplesProcessed += samplesToProcess;
    }

    auto end = std::chrono::high_resolution_clock::now();
    auto elapsed = std::chrono::duration_cast<std::chrono::microseconds>(end - start);

    double elapsedSeconds = elapsed.count() / 1000000.0;
    double samplesPerSecond = numSamples / elapsedSeconds;
    double realTimeFactor = (numSamples / sampleRate) / elapsedSeconds;
    double microsecondsPerSample = elapsed.count() / static_cast<double>(numSamples);

    std::cout << "Processed " << numSamples << " samples in "
              << elapsed.count() << " microseconds" << std::endl;
    std::cout << "Samples per second: " << std::fixed << std::setprecision(2)
              << samplesPerSecond << std::endl;
    std::cout << "Real-time factor: " << std::fixed << std::setprecision(2)
              << realTimeFactor << "x" << std::endl;
    std::cout << "Microseconds per sample: " << std::fixed << std::setprecision(2)
              << microsecondsPerSample << std::endl;
}

int main(int, char**)
{
    runBenchmark();
    return 0;
}
