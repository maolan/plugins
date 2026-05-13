//! Kick-specific SIMD routines.
//!
//! Uses runtime feature detection for AVX2, AVX, and SSE fallbacks.

#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
mod x86 {
    pub use std::arch::x86_64::*;
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use x86::*;

/// Apply gain to a buffer: `buf[i] *= gain`
pub fn mul_gain_inplace(buf: &mut [f32], gain: f32) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            mul_gain_inplace_avx(buf, gain);
            return;
        }
        if is_x86_feature_detected!("sse") {
            mul_gain_inplace_sse(buf, gain);
            return;
        }
    }
    mul_gain_inplace_scalar(buf, gain);
}

fn mul_gain_inplace_scalar(buf: &mut [f32], gain: f32) {
    for s in buf.iter_mut() {
        *s *= gain;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
unsafe fn mul_gain_inplace_sse(buf: &mut [f32], gain: f32) {
    let gain_vec = _mm_set1_ps(gain);
    let mut i = 0;
    while i + 4 <= buf.len() {
        let v = _mm_loadu_ps(buf.as_ptr().add(i));
        let r = _mm_mul_ps(v, gain_vec);
        _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 4;
    }
    for s in buf[i..].iter_mut() {
        *s *= gain;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
unsafe fn mul_gain_inplace_avx(buf: &mut [f32], gain: f32) {
    let gain_vec = _mm256_set1_ps(gain);
    let mut i = 0;
    while i + 8 <= buf.len() {
        let v = _mm256_loadu_ps(buf.as_ptr().add(i));
        let r = _mm256_mul_ps(v, gain_vec);
        _mm256_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 8;
    }
    // Process remaining with SSE or scalar
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    if is_x86_feature_detected!("sse") && i + 4 <= buf.len() {
        let gain_vec = _mm_set1_ps(gain);
        while i + 4 <= buf.len() {
            let v = _mm_loadu_ps(buf.as_ptr().add(i));
            let r = _mm_mul_ps(v, gain_vec);
            _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
            i += 4;
        }
    }
    for s in buf[i..].iter_mut() {
        *s *= gain;
    }
}

/// Apply hard clip to a buffer: `buf[i] = clamp(buf[i], -limit, +limit)`
pub fn clip_inplace(buf: &mut [f32], limit: f32) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            clip_inplace_avx(buf, limit);
            return;
        }
        if is_x86_feature_detected!("sse") {
            clip_inplace_sse(buf, limit);
            return;
        }
    }
    clip_inplace_scalar(buf, limit);
}

fn clip_inplace_scalar(buf: &mut [f32], limit: f32) {
    for s in buf.iter_mut() {
        *s = s.clamp(-limit, limit);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
unsafe fn clip_inplace_sse(buf: &mut [f32], limit: f32) {
    let neg = _mm_set1_ps(-limit);
    let pos = _mm_set1_ps(limit);
    let mut i = 0;
    while i + 4 <= buf.len() {
        let v = _mm_loadu_ps(buf.as_ptr().add(i));
        let v = _mm_max_ps(v, neg);
        let v = _mm_min_ps(v, pos);
        _mm_storeu_ps(buf.as_mut_ptr().add(i), v);
        i += 4;
    }
    for s in buf[i..].iter_mut() {
        *s = s.clamp(-limit, limit);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
unsafe fn clip_inplace_avx(buf: &mut [f32], limit: f32) {
    let neg = _mm256_set1_ps(-limit);
    let pos = _mm256_set1_ps(limit);
    let mut i = 0;
    while i + 8 <= buf.len() {
        let v = _mm256_loadu_ps(buf.as_ptr().add(i));
        let v = _mm256_max_ps(v, neg);
        let v = _mm256_min_ps(v, pos);
        _mm256_storeu_ps(buf.as_mut_ptr().add(i), v);
        i += 8;
    }
    if i + 4 <= buf.len() {
        let neg = _mm_set1_ps(-limit);
        let pos = _mm_set1_ps(limit);
        let v = _mm_loadu_ps(buf.as_ptr().add(i));
        let v = _mm_max_ps(v, neg);
        let v = _mm_min_ps(v, pos);
        _mm_storeu_ps(buf.as_mut_ptr().add(i), v);
        i += 4;
    }
    for s in buf[i..].iter_mut() {
        *s = s.clamp(-limit, limit);
    }
}
