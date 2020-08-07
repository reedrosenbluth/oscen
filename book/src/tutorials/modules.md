# Creating Your Own Synth Module

**Tutorial Info**

- Author: Jeffrey Rosenbluth
- Required Knowledge:
    - [Getting Started](/getting_started.md)

---

## Introduction

**Oscen** provides many pre-built synth modules, but eventually you might want
to build your own. In this tutorial we will build 3 synth modules:
1. A Funky Oscillator
2. A Leaky Integrator (i.e. a one pole low-pass filter)
3. A Soft Clipping Module

## Funky Oscillator

We will build an oscillator that is a sine wave for the first half of it's period
and a sawtooth wave for the second half. This is easier to easiest to describe
with a picture.

![A Funky Oscillator](../images/funky_osc.svg)

By a synth module what we mean as any struct that implements the
`Signal` trait. This trait has 3 methods two of which are basically
boiler plate that we create with the help of macros. The first method `as_any_mut`
is necessary to downcast the module so that if necessary we can access any special
field or features of the module. The second method `tag` is used to obtain the
unique identifier of this module so that it can be used as an input to another module.
In most cases both of these methods can be implmented via the `std_signal` macro.

Let's begin building the module, first we create the struct. Almost all synth
modules will have a tag field, which contains a unique identifier. We usually
initialize this using the `id()` function on a `IdGen` struct, which creates a unique integer id. 
We also need fields for the frequency `hz`, amplitutde and 
phase of the oscillator. These fields are all of type `In` which is an enum with
two variants: `Cv(Tag)` or `Fix(Real)`. The `In` enum allows these fields to be
set to a fixed number or controlled by another module. The constructor `new` 
initializes these fields to be `Fixed` by using the `into` method to convert numbers
to type `In`.

```rust,no_run
 use oscen::signal::*;
 use crate::{as_any_mut, std_signal};

 #[derive(Clone]
 pub struct Funky {
     tag: Tag,
     hz: In,
     amplitude: In,
     phase: Int,
 }

 impl Funky {
    pub fn new(id_gen: &mut IdGen) -> Self {
        Self {
            tag: id_gen.id(),
            hz: 0.into(),
            amplitude: 1.into(),
            phase: 0.into(),
        }
    }
```

Oscen uses a builder pattern to create modules and set their fields. The following
3 functions are used to set these fields and return the struct for further modification.
The `Builder` trait also has a few methods necessary for completing the build phase
and for adding the synth to a `rack`. The methods in this trait all have
default implementations which will work in just about all cases. You can see
what these methods do [here](https://docs.rs/oscen/0.1.4/oscen/signal/trait.Builder.html)

```rust,no_run
    // hz takes any type the implements Into<In> and uses this value to set
    // the hz field.
    pub fn hz<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.hz = arg.into();
        self
    }

    // Set the amplitude.
    pub fn amplitude<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.amplitude = arg.into();
        self
    }

    // Set the phase.
    pub fn phase<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.phase = arg.into();
        self
    }
}

impl Builder for Funky {} 
```

The most important method is the `signal` method itself, this is the funciton 
that produces a sample when called - every \\(\frac{1}{f_s}\\) seconds where \\(f_s\\) is the sample rate. The signal function is also
responsible for updating the phase of an oscillator (it's necessary for an 
oscillator to store it's phase to avoid clipping). The function `In::val` is
a convenience method of `In` to convert these fields to values.

```rust,no_run
impl Signal for Funky {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let hz = In::val(rack, self.hz);
        let amplitude = In::val(rack, self.amplitude);
        let phase = In::val(rack, self.phase);
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
        };
        if phase <= 0.5 {
            amplitude * (TAU * phase).sin()
        } else {
            amplitude * (1.0 - 2.0 * (phase % 1.0))
        }
    }
}
```
