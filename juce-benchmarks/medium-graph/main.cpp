/*
  JUCE Medium Graph Benchmark

  Matches oscen's medium_graph: 2 oscillators + filter + envelope

  Oscen equivalent:
  - Sine oscillator (440Hz, amp 1.0)
  - Saw oscillator (442Hz, amp 1.0)
  - TPT lowpass filter (1000Hz, Q 0.7)
  - ADSR envelope (0.01, 0.1, 0.7, 0.2)
  - Mix oscillators → filter → multiply by envelope
*/

#include <juce_core/juce_core.h>
#include <juce_audio_basics/juce_audio_basics.h>
#include <juce_audio_processors/juce_audio_processors.h>
#include <juce_dsp/juce_dsp.h>
#include <juce_events/juce_events.h>
#include <chrono>
#include <iostream>
#include <iomanip>

class MediumGraphProcessor : public juce::AudioProcessor
{
public:
    MediumGraphProcessor()
        : AudioProcessor(BusesProperties()
                        .withOutput("Output", juce::AudioChannelSet::mono(), true))
    {
    }

    const juce::String getName() const override { return "MediumGraph"; }
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

        // Oscillators
        sineOsc.prepare(spec);
        sineOsc.setFrequency(440.0f);

        sawOsc.prepare(spec);
        sawOsc.setFrequency(442.0f);

        // Filter (TPT lowpass, 1000Hz, Q=0.7)
        filter.prepare(spec);
        filter.reset();
        *filter.state = *juce::dsp::IIR::Coefficients<float>::makeLowPass(
            sampleRate, 1000.0f, 0.7f);

        // ADSR Envelope
        juce::ADSR::Parameters envParams;
        envParams.attack = 0.01f;
        envParams.decay = 0.1f;
        envParams.sustain = 0.7f;
        envParams.release = 0.2f;
        envelope.setParameters(envParams);
        envelope.setSampleRate(sampleRate);
        envelope.noteOn();  // Trigger envelope
    }

    void releaseResources() override {}

    void processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer&) override
    {
        auto numSamples = buffer.getNumSamples();

        // Process sine oscillator into buffer
        auto audioBlock = juce::dsp::AudioBlock<float>(buffer);
        auto context = juce::dsp::ProcessContextReplacing<float>(audioBlock);
        sineOsc.process(context);

        // Create temp buffer for saw oscillator
        juce::AudioBuffer<float> sawBuffer(1, numSamples);
        auto sawBlock = juce::dsp::AudioBlock<float>(sawBuffer);
        auto sawContext = juce::dsp::ProcessContextReplacing<float>(sawBlock);
        sawOsc.process(sawContext);

        // Mix oscillators
        buffer.addFrom(0, 0, sawBuffer, 0, 0, numSamples);

        // Apply filter
        filter.process(context);

        // Apply envelope
        auto* channelData = buffer.getWritePointer(0);
        for (int i = 0; i < numSamples; ++i)
        {
            float envValue = envelope.getNextSample();
            channelData[i] *= envValue;
        }
    }

    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    bool hasEditor() const override { return false; }

    void getStateInformation(juce::MemoryBlock&) override {}
    void setStateInformation(const void*, int) override {}

private:
    juce::dsp::Oscillator<float> sineOsc { [](float x) { return std::sin(x); } };
    juce::dsp::Oscillator<float> sawOsc { [](float x) { return x / juce::MathConstants<float>::pi; } };
    juce::dsp::ProcessorDuplicator<juce::dsp::IIR::Filter<float>,
                                     juce::dsp::IIR::Coefficients<float>> filter;
    juce::ADSR envelope;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(MediumGraphProcessor)
};

void runBenchmark()
{
    const int numSamples = 441000;  // 10 seconds at 44.1kHz
    const double sampleRate = 44100.0;
    const int blockSize = 512;

    MediumGraphProcessor processor;
    processor.setRateAndBufferSizeDetails(sampleRate, blockSize);
    processor.prepareToPlay(sampleRate, blockSize);

    juce::AudioBuffer<float> buffer(1, blockSize);
    juce::MidiBuffer midiBuffer;

    std::cout << "=== JUCE Medium Graph (2 osc + filter + env) ===" << std::endl;
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

int main(int, char**)
{
    juce::ScopedJuceInitialiser_GUI juceInit;
    runBenchmark();
    return 0;
}
