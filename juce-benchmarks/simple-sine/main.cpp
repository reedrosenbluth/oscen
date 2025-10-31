/*
  JUCE Simple Sine Benchmark

  Benchmarks a single sine oscillator to compare with oscen's simple_graph benchmark.

  Run with: ./simple-sine
  Or for profiling: perf record --call-graph=dwarf ./simple-sine
*/

#include <juce_core/juce_core.h>
#include <juce_audio_basics/juce_audio_basics.h>
#include <juce_audio_processors/juce_audio_processors.h>
#include <juce_dsp/juce_dsp.h>
#include <juce_events/juce_events.h>
#include <chrono>
#include <iostream>
#include <iomanip>

class SimpleSineProcessor : public juce::AudioProcessor
{
public:
    SimpleSineProcessor()
        : AudioProcessor(BusesProperties()
                        .withOutput("Output", juce::AudioChannelSet::mono(), true))
    {
    }

    const juce::String getName() const override { return "SimpleSine"; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    double getTailLengthSeconds() const override { return 0.0; }
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram(int) override {}
    const juce::String getProgramName(int) override { return {}; }
    void changeProgramName(int, const juce::String&) override {}

    void prepareToPlay(double sampleRate, int samplesPerBlock) override
    {
        juce::dsp::ProcessSpec spec;
        spec.sampleRate = sampleRate;
        spec.maximumBlockSize = static_cast<juce::uint32>(samplesPerBlock);
        spec.numChannels = 1;

        oscillator.prepare(spec);
        oscillator.setFrequency(440.0f);
    }

    void releaseResources() override {}

    void processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer&) override
    {
        auto audioBlock = juce::dsp::AudioBlock<float>(buffer);
        auto context = juce::dsp::ProcessContextReplacing<float>(audioBlock);
        oscillator.process(context);
    }

    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    bool hasEditor() const override { return false; }

    void getStateInformation(juce::MemoryBlock&) override {}
    void setStateInformation(const void*, int) override {}

private:
    juce::dsp::Oscillator<float> oscillator { [](float x) { return std::sin(x); } };

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(SimpleSineProcessor)
};

// Benchmark function that processes samples without audio I/O
void runBenchmark()
{
    const int numSamples = 441000;  // 10 seconds at 44.1kHz
    const double sampleRate = 44100.0;
    const int blockSize = 512;

    SimpleSineProcessor processor;
    processor.setRateAndBufferSizeDetails(sampleRate, blockSize);
    processor.prepareToPlay(sampleRate, blockSize);

    juce::AudioBuffer<float> buffer(1, blockSize);
    juce::MidiBuffer midiBuffer;

    std::cout << "=== JUCE Simple Sine (1 oscillator) ===" << std::endl;
    std::cout << "Processing " << numSamples << " samples..." << std::endl;

    auto start = std::chrono::high_resolution_clock::now();

    int samplesProcessed = 0;
    while (samplesProcessed < numSamples)
    {
        int samplesToProcess = std::min(blockSize, numSamples - samplesProcessed);
        buffer.setSize(1, samplesToProcess, false, false, true);

        processor.processBlock(buffer, midiBuffer);
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

int main(int argc, char* argv[])
{
    juce::ScopedJuceInitialiser_GUI juceInit;
    runBenchmark();
    return 0;
}
