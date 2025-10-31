/*
  JUCE Complex Graph Benchmark

  Matches oscen's complex_graph: 5 oscillators + 2 filters + 2 envelopes + delay

  Oscen equivalent:
  - 5 oscillators: sine(440, 0.3), saw(450, 0.3), sine(460, 0.3), saw(470, 0.3), sine(480, 0.3)
  - Mix first 3 → filter1 (800Hz, Q 0.5) → multiply by env1 (0.01, 0.1, 0.7, 0.2)
  - Mix last 2 → filter2 (1200Hz, Q 0.5) → multiply by env2 (0.02, 0.15, 0.6, 0.3)
  - Mix both → delay (0.5s, feedback 0.3)
*/

#include <juce_core/juce_core.h>
#include <juce_audio_basics/juce_audio_basics.h>
#include <juce_audio_processors/juce_audio_processors.h>
#include <juce_dsp/juce_dsp.h>
#include <juce_events/juce_events.h>
#include <chrono>
#include <iostream>
#include <iomanip>

class ComplexGraphProcessor : public juce::AudioProcessor
{
public:
    ComplexGraphProcessor()
        : AudioProcessor(BusesProperties()
                        .withOutput("Output", juce::AudioChannelSet::mono(), true))
    {
    }

    const juce::String getName() const override { return "ComplexGraph"; }
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

        // 5 Oscillators
        osc1.prepare(spec);
        osc1.setFrequency(440.0f);

        osc2.prepare(spec);
        osc2.setFrequency(450.0f);

        osc3.prepare(spec);
        osc3.setFrequency(460.0f);

        osc4.prepare(spec);
        osc4.setFrequency(470.0f);

        osc5.prepare(spec);
        osc5.setFrequency(480.0f);

        // 2 Filters
        filter1.prepare(spec);
        filter1.reset();
        *filter1.state = *juce::dsp::IIR::Coefficients<float>::makeLowPass(
            sampleRate, 800.0f, 0.5f);

        filter2.prepare(spec);
        filter2.reset();
        *filter2.state = *juce::dsp::IIR::Coefficients<float>::makeLowPass(
            sampleRate, 1200.0f, 0.5f);

        // 2 ADSR Envelopes
        juce::ADSR::Parameters env1Params;
        env1Params.attack = 0.01f;
        env1Params.decay = 0.1f;
        env1Params.sustain = 0.7f;
        env1Params.release = 0.2f;
        envelope1.setParameters(env1Params);
        envelope1.setSampleRate(sampleRate);
        envelope1.noteOn();

        juce::ADSR::Parameters env2Params;
        env2Params.attack = 0.02f;
        env2Params.decay = 0.15f;
        env2Params.sustain = 0.6f;
        env2Params.release = 0.3f;
        envelope2.setParameters(env2Params);
        envelope2.setSampleRate(sampleRate);
        envelope2.noteOn();

        // Delay (0.5 seconds)
        delay.prepare(spec);
        delay.reset();
        delay.setMaximumDelayInSamples(static_cast<int>(sampleRate * 0.5f));
        delay.setDelay(sampleRate * 0.5f);

        // Allocate temp buffers
        tempBuffer1.setSize(1, samplesPerBlock);
        tempBuffer2.setSize(1, samplesPerBlock);
        tempBuffer3.setSize(1, samplesPerBlock);
        tempBuffer4.setSize(1, samplesPerBlock);
        tempBuffer5.setSize(1, samplesPerBlock);
    }

    void releaseResources() override {}

    void processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer&) override
    {
        auto numSamples = buffer.getNumSamples();

        // Process oscillator 1 (sine) into main buffer
        auto audioBlock = juce::dsp::AudioBlock<float>(buffer);
        auto context = juce::dsp::ProcessContextReplacing<float>(audioBlock);
        osc1.process(context);
        buffer.applyGain(0.3f);  // Amplitude 0.3

        // Process oscillators 2-5 into temp buffers
        auto block2 = juce::dsp::AudioBlock<float>(tempBuffer1).getSubBlock(0, numSamples);
        auto context2 = juce::dsp::ProcessContextReplacing<float>(block2);
        osc2.process(context2);
        tempBuffer1.applyGain(0, 0, numSamples, 0.3f);

        auto block3 = juce::dsp::AudioBlock<float>(tempBuffer2).getSubBlock(0, numSamples);
        auto context3 = juce::dsp::ProcessContextReplacing<float>(block3);
        osc3.process(context3);
        tempBuffer2.applyGain(0, 0, numSamples, 0.3f);

        auto block4 = juce::dsp::AudioBlock<float>(tempBuffer3).getSubBlock(0, numSamples);
        auto context4 = juce::dsp::ProcessContextReplacing<float>(block4);
        osc4.process(context4);
        tempBuffer3.applyGain(0, 0, numSamples, 0.3f);

        auto block5 = juce::dsp::AudioBlock<float>(tempBuffer4).getSubBlock(0, numSamples);
        auto context5 = juce::dsp::ProcessContextReplacing<float>(block5);
        osc5.process(context5);
        tempBuffer4.applyGain(0, 0, numSamples, 0.3f);

        // Mix first 3 oscillators into buffer
        buffer.addFrom(0, 0, tempBuffer1, 0, 0, numSamples);  // osc1 + osc2
        buffer.addFrom(0, 0, tempBuffer2, 0, 0, numSamples);  // + osc3

        // Apply filter1 to first mix
        filter1.process(context);

        // Mix last 2 oscillators into tempBuffer5
        tempBuffer5.copyFrom(0, 0, tempBuffer3, 0, 0, numSamples);  // osc4
        tempBuffer5.addFrom(0, 0, tempBuffer4, 0, 0, numSamples);   // + osc5

        // Apply filter2 to second mix
        auto block6 = juce::dsp::AudioBlock<float>(tempBuffer5).getSubBlock(0, numSamples);
        auto context6 = juce::dsp::ProcessContextReplacing<float>(block6);
        filter2.process(context6);

        // Apply envelopes
        auto* channelData1 = buffer.getWritePointer(0);
        auto* channelData2 = tempBuffer5.getWritePointer(0);
        for (int i = 0; i < numSamples; ++i)
        {
            float env1Value = envelope1.getNextSample();
            float env2Value = envelope2.getNextSample();
            channelData1[i] *= env1Value;
            channelData2[i] *= env2Value;
        }

        // Mix both filtered/enveloped signals
        buffer.addFrom(0, 0, tempBuffer5, 0, 0, numSamples);

        // Apply delay with feedback
        auto* channelData = buffer.getWritePointer(0);
        for (int i = 0; i < numSamples; ++i)
        {
            float input = channelData[i];
            float delayedSample = delay.popSample(0);
            float output = input + (delayedSample * 0.3f);  // 0.3 feedback
            delay.pushSample(0, output);
            channelData[i] = output;
        }
    }

    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    bool hasEditor() const override { return false; }

    void getStateInformation(juce::MemoryBlock&) override {}
    void setStateInformation(const void*, int) override {}

private:
    // 5 Oscillators
    juce::dsp::Oscillator<float> osc1 { [](float x) { return std::sin(x); } };
    juce::dsp::Oscillator<float> osc2 { [](float x) { return x / juce::MathConstants<float>::pi; } };
    juce::dsp::Oscillator<float> osc3 { [](float x) { return std::sin(x); } };
    juce::dsp::Oscillator<float> osc4 { [](float x) { return x / juce::MathConstants<float>::pi; } };
    juce::dsp::Oscillator<float> osc5 { [](float x) { return std::sin(x); } };

    // 2 Filters
    juce::dsp::ProcessorDuplicator<juce::dsp::IIR::Filter<float>,
                                     juce::dsp::IIR::Coefficients<float>> filter1;
    juce::dsp::ProcessorDuplicator<juce::dsp::IIR::Filter<float>,
                                     juce::dsp::IIR::Coefficients<float>> filter2;

    // 2 Envelopes
    juce::ADSR envelope1;
    juce::ADSR envelope2;

    // Delay
    juce::dsp::DelayLine<float, juce::dsp::DelayLineInterpolationTypes::Linear> delay;

    // Temp buffers for mixing
    juce::AudioBuffer<float> tempBuffer1;
    juce::AudioBuffer<float> tempBuffer2;
    juce::AudioBuffer<float> tempBuffer3;
    juce::AudioBuffer<float> tempBuffer4;
    juce::AudioBuffer<float> tempBuffer5;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(ComplexGraphProcessor)
};

void runBenchmark()
{
    const int numSamples = 441000;  // 10 seconds at 44.1kHz
    const double sampleRate = 44100.0;
    const int blockSize = 512;

    ComplexGraphProcessor processor;
    processor.setRateAndBufferSizeDetails(sampleRate, blockSize);
    processor.prepareToPlay(sampleRate, blockSize);

    juce::AudioBuffer<float> buffer(1, blockSize);
    juce::MidiBuffer midiBuffer;

    std::cout << "=== JUCE Complex Graph (5 osc + 2 filters + 2 env + delay) ===" << std::endl;
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
