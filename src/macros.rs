/// Creates a public struct and makes all fields public.
#[macro_export]
macro_rules! pub_struct {
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

    (
        $(#[$outer:meta])*
        struct $name:ident($head:ty$(, $last:ty)*)
    ) => {
        $(#[$outer:meta])*
        pub struct $name(pub $head$(, pub $last)*);
    };
}

/// Creates a wave with prepopulated boilerplate code for calling the
/// `WaveParams` methods. This shouldn't be used for any waves that need to
/// customize anything more than the sample function.
#[macro_export]
macro_rules! basic_wave {
    ($wave:ident, $sample:expr) => {
        pub struct $wave($crate::WaveParams);

        impl $wave {
            pub fn new(hz: f64, volume: f32) -> Self {
                $wave($crate::WaveParams::new(hz, volume))
            }
        }

        impl Wave for $wave {
            fn sample(&self) -> f32 {
                $sample(self)
            }

            fn update_phase(&mut self, sample_rate: f64) {
                self.0.update_phase(sample_rate)
            }

            fn mul_hz(&mut self, factor: f64) {
                self.0.mul_hz(factor);
            }

            fn mod_hz(&mut self, factor: f64) {
                self.0.mod_hz(factor);
            }
        }
    };
}
