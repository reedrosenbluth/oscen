/// Creates a public struct and makes all fields public.
#[macro_export]
macro_rules! pub_struct {
    // Regular Struct
    (
        $(#[$outer:meta])*
        struct $name:ident {
            $(
                $(#[$inner:ident $($args:tt)*])*
                $field:ident: $t:ty,
            )*
        }
    ) => {
        $(#[$outer])*
        pub struct $name {
            $(
                $(#[$inner $($args)*])*
                pub $field: $t
            ),*
        }
    };

    // Tuple Struct
    (
        $(#[$outer:meta])*
        struct $name:ident($head:ty$(, $last:ty)*)
    ) => {
        $(#[$outer:meta])*
        pub struct $name(pub $head$(, pub $last)*);
    };

    // Unit Struct
    (
        $(#[$outer:meta])*
        struct $name:ident
    ) => {
        $(#[$outer:meta])*
        pub struct $name;
    };
}

/// Creates a wave with prepopulated boilerplate code for calling the
/// `WaveParams` methods. This shouldn't be used for any waves that need to
/// customize anything more than the sample function.
#[macro_export]
macro_rules! basic_wave {
    ($wave:ident, $sample:expr) => {
        #[derive(Clone)]
        pub struct $wave(pub $crate::dsp::WaveParams);

        impl $wave {
            pub fn new(hz: f64) -> Self {
                $wave($crate::dsp::WaveParams::new(hz))
            }

            pub fn boxed(hz: f64) -> ArcMutex<Self> {
                arc($wave($crate::dsp::WaveParams::new(hz)))
            }
        }

        impl Wave for $wave {
            fn sample(&self) -> f32 {
                $sample(self)
            }

            fn update_phase(&mut self, sample_rate: f64) {
                self.0.update_phase(sample_rate)
            }
        }
    };
}