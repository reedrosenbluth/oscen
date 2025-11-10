/*
  Cmajor Complex Graph Benchmark

  Matches oscen's complex_graph: 5 oscillators + 2 filters + 2 envelopes + delay

  Run with: ./complex-graph
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

    auto engine = cmaj::Engine::create();

    if (!engine)
    {
        std::cerr << "Failed to create Cmajor engine" << std::endl;
        return;
    }

    std::string cmajorCode = loadFile("ComplexGraph.cmajor");

    cmaj::ProgramInterface program;
    program.name = "ComplexGraph";
    program.mainProcessor = "ComplexGraph";

    auto buildSettings = engine->createBuildSettings();
    buildSettings->setFrequency(sampleRate);
    buildSettings->setBlockSize(blockSize);

    auto compileResult = engine->load(program, cmajorCode.c_str(), buildSettings);

    if (!compileResult.isOK())
    {
        std::cerr << "Failed to compile Cmajor program: "
                  << compileResult.getErrorMessage() << std::endl;
        return;
    }

    if (!engine->link().isOK())
    {
        std::cerr << "Failed to link program" << std::endl;
        return;
    }

    std::vector<float> outputBuffer(blockSize);

    std::cout << "=== Cmajor Complex Graph (5 osc + 2 filters + 2 env + delay) ===" << std::endl;
    std::cout << "Processing " << numSamples << " samples..." << std::endl;

    auto start = std::chrono::high_resolution_clock::now();

    int samplesProcessed = 0;
    while (samplesProcessed < numSamples)
    {
        int samplesToProcess = std::min(blockSize, numSamples - samplesProcessed);
        engine->advance();
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
