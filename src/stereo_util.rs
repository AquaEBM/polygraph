use core::mem::transmute;

use plugin_util::{
    simd::{SimdElement, Simd, simd_swizzle, prelude::SimdFloat},
    simd_util::{Float, FLOATS_PER_VECTOR, const_splat},
    math::exp2,
};

pub const STEREO_VOICES_PER_VECTOR: usize = FLOATS_PER_VECTOR / 2;

// Safety argument for the following two functions:
//  - both referred to types have the same size, more specifically, 2 * STEREO_VOICES_PER_VECTOR
// is always equal to FLOATS_PER_VECTOR, because it is always a multiple (in fact, a power) of 2
//  - the type of `vector` has greater alignment that of the return type
//  - the output reference's lifetime is the same as that of the input, so no unbounded lifetimes
//  - we are transmuting a vector to an array over the same scalar, so values are valid

#[inline]
pub fn as_stereo_sample_array<'a, T: SimdElement>(
    vector: &'a Simd<T, FLOATS_PER_VECTOR>,
) -> &'a [Simd<T, 2>; STEREO_VOICES_PER_VECTOR] {
    unsafe { transmute(vector) }
}

#[inline]
pub fn as_mut_stereo_sample_array<'a, T: SimdElement>(
    vector: &'a mut Simd<T, FLOATS_PER_VECTOR>,
) -> &mut [Simd<T, 2>; STEREO_VOICES_PER_VECTOR] {
    unsafe { transmute(vector) }
}

#[inline]
pub fn splat_stereo<T: SimdElement>(pair: Simd<T, 2>) -> Simd<T, FLOATS_PER_VECTOR> {
    const ZERO_ONE: [usize; FLOATS_PER_VECTOR] = {
        let mut array = [0; FLOATS_PER_VECTOR];
        let mut i = 1;
        while i < FLOATS_PER_VECTOR {
            array[i] = 1;
            i += 2;
        }
        array
    };

    simd_swizzle!(pair, ZERO_ONE)
}

/// return a vector where values on the left channel
/// are on the right ones and vice-versa
#[inline]
pub fn swap_stereo(v: Float) -> Float {
    const FLIP_PAIRS: [usize; FLOATS_PER_VECTOR] = {
        let mut array = [0; FLOATS_PER_VECTOR];

        let mut i = 0;
        while i < FLOATS_PER_VECTOR {
            array[i] = i ^ 1;
            i += 1;
        }
        array
    };

    simd_swizzle!(v, FLIP_PAIRS)
}

#[inline]
pub fn semitones_to_ratio(semitones: Float) -> Float {
    const RATIO: Float = const_splat(1. / 12.);
    exp2(semitones * RATIO)
}

/// triangluar panning of a vector of stereo samples, 0 < pan <= 1
#[inline]
pub fn triangular_pan_weights(pan_norm: Float) -> Float {
    const SIGN_MASK: Float = {
        let mut array = [0.; FLOATS_PER_VECTOR];
        let mut i = 0;
        while i < FLOATS_PER_VECTOR {
            array[i] = -0.;
            i += 2;
        }
        Simd::from_array(array)
    };

    const ALT_ONE: Float = {
        let mut array = [0.; FLOATS_PER_VECTOR];
        let mut i = 0;
        while i < FLOATS_PER_VECTOR {
            array[i] = 1.;
            i += 2;
        }
        Simd::from_array(array)
    };

    Float::from_bits(pan_norm.to_bits() ^ SIGN_MASK.to_bits()) + ALT_ONE
}

#[inline]
pub fn splat_slot<T: SimdElement>(
    vector: &Simd<T, FLOATS_PER_VECTOR>,
    index: usize,
) -> Option<Simd<T, FLOATS_PER_VECTOR>> {
    let array = as_stereo_sample_array(vector);

    array.get(index).copied().map(splat_stereo)
}