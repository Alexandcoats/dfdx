use core::cmp::Ordering;

use crate::shapes::Unit;

/// Error while creating a u4, because the value was out of range and could not be converted.
#[derive(Debug, Clone, Copy)]
pub struct HalfByteConstructionError;

impl std::fmt::Display for HalfByteConstructionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "failed to construct a half byte value")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for HalfByteConstructionError {}

/// A Half-byte value. This type exists only for validation purposes, to ensure that
/// values that get stores are within the range [0..16)
#[allow(non_camel_case_types)]
pub struct u4(pub(crate) u8);

impl u4 {
    fn from_byte(byte_value: u8) -> Result<Self, HalfByteConstructionError> {
        if byte_value > 0b_0000_1111 {
            Err(HalfByteConstructionError)
        } else {
            Ok(Self(byte_value))
        }
    }
}

/// Implementation of the Q4_0 quantization method outlined [here](https://github.com/ggerganov/ggml/pull/27#).
#[derive(Copy, Clone, Debug)]
pub struct ScaledQuant<E> {
    scaling_factor: E,
}

/// Implementation of the Q4_1 quantization method outlined [here](https://github.com/ggerganov/ggml/pull/27#).
#[derive(Copy, Clone, Debug)]
pub struct OffsetQuant<E> {
    scaling_factor: E,
    offset_factor: E,
}

/// Trait that defines how to quantize a value.
pub trait Quantize: Clone {
    type Value: num_traits::Float + Unit;

    fn from_values(values: &[Self::Value]) -> Self
    where
        Self: Sized;

    fn quantize(&self, value: Self::Value) -> u4;

    fn dequantize(&self, half_byte: u4) -> Self::Value;
}

impl<E: num_traits::Float + Unit> Quantize for ScaledQuant<E> {
    type Value = E;

    fn from_values(values: &[E]) -> Self {
        Self {
            scaling_factor: values
                .iter()
                .max_by(|f1, f2| f1.partial_cmp(f2).unwrap())
                .unwrap()
                .abs()
                / E::from(7.0).unwrap(),
        }
    }

    fn quantize(&self, value: E) -> u4 {
        let inv_scaling_factor = if self.scaling_factor.is_zero() {
            E::zero()
        } else {
            E::one() / self.scaling_factor
        };
        u4::from_byte(
            ((value * inv_scaling_factor).round().to_f32().unwrap() as i8 + 8)
                .min(15)
                .max(0) as u8,
        )
        .unwrap()
    }

    fn dequantize(&self, half_byte: u4) -> E {
        E::from(half_byte.0 as i8 - 8).unwrap() * self.scaling_factor
    }
}

impl<E: num_traits::Float + Unit> Quantize for OffsetQuant<E> {
    type Value = E;

    fn from_values(values: &[E]) -> Self {
        let (mut min, mut max) = (E::max_value(), E::min_value());
        for v in values.iter() {
            if matches!(min.partial_cmp(v), Some(Ordering::Greater)) {
                min = *v;
            }
            if matches!(max.partial_cmp(v), Some(Ordering::Less)) {
                max = *v;
            }
        }
        Self {
            scaling_factor: (max - min) / E::from(15.0).unwrap(),
            offset_factor: min,
        }
    }

    fn quantize(&self, value: E) -> u4 {
        let inv_scaling_factor = if self.scaling_factor.is_zero() {
            E::zero()
        } else {
            E::one() / self.scaling_factor
        };
        u4::from_byte(
            (((value - self.offset_factor) * inv_scaling_factor)
                .round()
                .to_f32()
                .unwrap() as u8)
                .min(15)
                .max(0),
        )
        .unwrap()
    }

    fn dequantize(&self, half_byte: u4) -> E {
        E::from(half_byte.0).unwrap() * self.scaling_factor + self.offset_factor
    }
}

#[cfg(test)]
mod test {
    use crate::{prelude::*, tests::TestDtype};

    use super::*;

    #[test]
    fn test_round_trip_offset() {
        let dev = QuantizedCpu::<OffsetQuant<TestDtype>>::default();
        // Strangely, if I make the tensor just a little bigger (ex. 640x640)
        // it will fail below with a stack overflow, and if I make it a lot bigger
        // (ex. 640x6400) it will fail here with SIGSEGV: invalid memory reference
        let t: Tensor<Rank2<320, 640>, TestDtype, _> = dev.sample_normal();
        let size_t = t.data.size();
        println!("Quantized bytes: {}", size_t);
        let mut v = t.as_vec();
        for val in v.iter_mut() {
            *val = val.abs().powf(1.4).tanh() + 3.0;
        }
        let size_v =
            std::mem::size_of::<Vec<TestDtype>>() + v.capacity() * std::mem::size_of::<TestDtype>();
        println!("Vec bytes: {}", size_v);
        let t2: Tensor<Rank2<320, 640>, TestDtype, _> = dev.tensor(v);
        let errs = (t.abs().powf(1.4).tanh() + 3.0)
            .iter()
            .zip(t2.iter())
            .map(|(v1, v2)| (v1 - v2).abs())
            .collect::<Vec<_>>();
        let avg_err = errs.iter().sum::<TestDtype>() / errs.len() as TestDtype;
        let max_err = *errs
            .iter()
            .max_by(|f1, f2| f1.partial_cmp(f2).unwrap())
            .unwrap();
        assert!(avg_err < 0.05);
        assert!(max_err < 0.1);
    }

    #[test]
    fn test_round_trip_scaled() {
        let dev = QuantizedCpu::<ScaledQuant<TestDtype>>::default();
        // Strangely, if I make the tensor just a little bigger (ex. 640x640)
        // it will fail below with a stack overflow, and if I make it a lot bigger
        // (ex. 640x6400) it will fail here with SIGSEGV: invalid memory reference
        let t: Tensor<Rank2<320, 640>, TestDtype, _> = dev.sample_normal();
        let mut v = t.as_vec();
        for val in v.iter_mut() {
            *val = val.abs().powf(1.4).tanh() + 3.0;
        }
        let t2: Tensor<Rank2<320, 640>, TestDtype, _> = dev.tensor(v);
        let errs = (t.abs().powf(1.4).tanh() + 3.0)
            .iter()
            .zip(t2.iter())
            .map(|(v1, v2)| (v1 - v2).abs())
            .collect::<Vec<_>>();
        let avg_err = errs.iter().sum::<TestDtype>() / errs.len() as TestDtype;
        let max_err = *errs
            .iter()
            .max_by(|f1, f2| f1.partial_cmp(f2).unwrap())
            .unwrap();
        assert!(avg_err < 0.5);
        assert!(max_err < 1.0);
    }
}
