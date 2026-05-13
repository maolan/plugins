//! Portable SIMD helper routines for buffer math (plugin-local copy).

#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
mod x86 {
    pub use std::arch::x86_64::*;
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use x86::*;

/// dst[i] += src[i]
pub fn add_inplace(dst: &mut [f32], src: &[f32]) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            add_inplace_avx(dst, src);
            return;
        }
        if is_x86_feature_detected!("sse") {
            add_inplace_sse(dst, src);
            return;
        }
    }
    add_inplace_scalar(dst, src);
}

/// dst[i] += src[i] * gain
pub fn add_scaled_inplace(dst: &mut [f32], src: &[f32], gain: f32) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") && is_x86_feature_detected!("fma") {
            add_scaled_inplace_avx_fma(dst, src, gain);
            return;
        }
        if is_x86_feature_detected!("avx") {
            add_scaled_inplace_avx(dst, src, gain);
            return;
        }
        if is_x86_feature_detected!("sse") {
            add_scaled_inplace_sse(dst, src, gain);
            return;
        }
    }
    add_scaled_inplace_scalar(dst, src, gain);
}

/// dst[i] = dst[i] * scale[i] + loc[i]
pub fn affine_inplace(dst: &mut [f32], scale: &[f32], loc: &[f32]) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") && is_x86_feature_detected!("fma") {
            affine_inplace_avx_fma(dst, scale, loc);
            return;
        }
        if is_x86_feature_detected!("avx") {
            affine_inplace_avx(dst, scale, loc);
            return;
        }
        if is_x86_feature_detected!("sse") {
            affine_inplace_sse(dst, scale, loc);
            return;
        }
    }
    affine_inplace_scalar(dst, scale, loc);
}

/// Horizontal max of abs(buf[i]).
pub fn peak_abs(buf: &[f32]) -> f32 {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            return peak_abs_avx(buf);
        }
        if is_x86_feature_detected!("sse") {
            return peak_abs_sse(buf);
        }
    }
    peak_abs_scalar(buf)
}

/// dst[i] *= src[i]
pub fn mul_per_sample_inplace(dst: &mut [f32], src: &[f32]) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            mul_per_sample_inplace_avx(dst, src);
            return;
        }
        if is_x86_feature_detected!("sse") {
            mul_per_sample_inplace_sse(dst, src);
            return;
        }
    }
    mul_per_sample_inplace_scalar(dst, src);
}

/// sum(a[i] * b[i])
pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") && is_x86_feature_detected!("fma") {
            return dot_product_avx_fma(a, b);
        }
        if is_x86_feature_detected!("avx") {
            return dot_product_avx(a, b);
        }
        if is_x86_feature_detected!("sse") {
            return dot_product_sse(a, b);
        }
    }
    dot_product_scalar(a, b)
}

fn add_scaled_inplace_scalar(dst: &mut [f32], src: &[f32], gain: f32) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d += *s * gain;
    }
}

/// dst[i] = src[i] * gain
pub fn copy_scaled_inplace(dst: &mut [f32], src: &[f32], gain: f32) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            copy_scaled_inplace_avx(dst, src, gain);
            return;
        }
        if is_x86_feature_detected!("sse") {
            copy_scaled_inplace_sse(dst, src, gain);
            return;
        }
    }
    copy_scaled_inplace_scalar(dst, src, gain);
}

fn copy_scaled_inplace_scalar(dst: &mut [f32], src: &[f32], gain: f32) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = *s * gain;
    }
}

fn affine_inplace_scalar(dst: &mut [f32], scale: &[f32], loc: &[f32]) {
    for ((d, s), l) in dst.iter_mut().zip(scale.iter()).zip(loc.iter()) {
        *d = *d * *s + *l;
    }
}

fn dot_product_scalar(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn peak_abs_scalar(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0f32, |acc, s| acc.max(s.abs()))
}

fn mul_per_sample_inplace_scalar(dst: &mut [f32], src: &[f32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d *= *s;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn peak_abs_sse(buf: &[f32]) -> f32 {
    let sign_mask = _mm_set1_ps(-0.0);
    let mut peak = _mm_setzero_ps();
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        let v = _mm_loadu_ps(buf.as_ptr().add(i));
        let abs_v = _mm_andnot_ps(sign_mask, v);
        peak = _mm_max_ps(peak, abs_v);
        i += 4;
    }
    let mut arr = [0.0f32; 4];
    _mm_storeu_ps(arr.as_mut_ptr(), peak);
    let mut max_scalar = arr.into_iter().fold(0.0f32, |a, b| a.max(b));
    for s in &buf[i..] {
        max_scalar = max_scalar.max(s.abs());
    }
    max_scalar
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn mul_per_sample_inplace_sse(dst: &mut [f32], src: &[f32]) {
    let len = dst.len().min(src.len());
    let mut i = 0usize;
    while i + 4 <= len {
        let d = _mm_loadu_ps(dst.as_ptr().add(i));
        let s = _mm_loadu_ps(src.as_ptr().add(i));
        let r = _mm_mul_ps(d, s);
        _mm_storeu_ps(dst.as_mut_ptr().add(i), r);
        i += 4;
    }
    for (d, s) in dst[i..len].iter_mut().zip(src[i..len].iter()) {
        *d *= *s;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn peak_abs_avx(buf: &[f32]) -> f32 {
    let mut peak = _mm256_setzero_ps();
    let mut i = 0;
    while i + 8 <= buf.len() {
        let v = _mm256_loadu_ps(buf.as_ptr().add(i));
        let abs_v = _mm256_andnot_ps(_mm256_set1_ps(-0.0), v);
        peak = _mm256_max_ps(peak, abs_v);
        i += 8;
    }
    let mut arr = [0.0f32; 8];
    _mm256_storeu_ps(arr.as_mut_ptr(), peak);
    let mut max_scalar = arr.iter().fold(0.0f32, |a, &b| a.max(b));
    for s in &buf[i..] {
        max_scalar = max_scalar.max(s.abs());
    }
    max_scalar
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn mul_per_sample_inplace_avx(dst: &mut [f32], src: &[f32]) {
    let len = dst.len().min(src.len());
    let mut i = 0;
    while i + 8 <= len {
        let d = _mm256_loadu_ps(dst.as_ptr().add(i));
        let s = _mm256_loadu_ps(src.as_ptr().add(i));
        let r = _mm256_mul_ps(d, s);
        _mm256_storeu_ps(dst.as_mut_ptr().add(i), r);
        i += 8;
    }
    for (d, s) in dst[i..len].iter_mut().zip(src[i..len].iter()) {
        *d *= *s;
    }
}

fn add_inplace_scalar(dst: &mut [f32], src: &[f32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d += *s;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn add_inplace_sse(dst: &mut [f32], src: &[f32]) {
    let len = dst.len().min(src.len());
    let dst_head = &mut dst[..len];
    let src_head = &src[..len];
    let mut i = 0usize;
    while i + 4 <= dst_head.len() {
        let d = _mm_loadu_ps(dst_head.as_ptr().add(i));
        let s = _mm_loadu_ps(src_head.as_ptr().add(i));
        let r = _mm_add_ps(d, s);
        _mm_storeu_ps(dst_head.as_mut_ptr().add(i), r);
        i += 4;
    }
    for (d, s) in dst_head[i..].iter_mut().zip(src_head[i..].iter()) {
        *d += *s;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn add_inplace_avx(dst: &mut [f32], src: &[f32]) {
    let len = dst.len().min(src.len());
    let dst_head = &mut dst[..len];
    let src_head = &src[..len];
    let mut i = 0;
    while i + 8 <= dst_head.len() {
        let d = _mm256_loadu_ps(dst_head.as_ptr().add(i));
        let s = _mm256_loadu_ps(src_head.as_ptr().add(i));
        let r = _mm256_add_ps(d, s);
        _mm256_storeu_ps(dst_head.as_mut_ptr().add(i), r);
        i += 8;
    }
    for (d, s) in dst_head[i..].iter_mut().zip(src_head[i..].iter()) {
        *d += *s;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn add_scaled_inplace_sse(dst: &mut [f32], src: &[f32], gain: f32) {
    let len = dst.len().min(src.len());
    let dst_head = &mut dst[..len];
    let src_head = &src[..len];
    let g = _mm_set1_ps(gain);
    let mut i = 0usize;
    while i + 4 <= dst_head.len() {
        let d = _mm_loadu_ps(dst_head.as_ptr().add(i));
        let s = _mm_loadu_ps(src_head.as_ptr().add(i));
        let r = _mm_add_ps(d, _mm_mul_ps(s, g));
        _mm_storeu_ps(dst_head.as_mut_ptr().add(i), r);
        i += 4;
    }
    for (d, s) in dst_head[i..].iter_mut().zip(src_head[i..].iter()) {
        *d += *s * gain;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn copy_scaled_inplace_sse(dst: &mut [f32], src: &[f32], gain: f32) {
    let len = dst.len().min(src.len());
    let dst_head = &mut dst[..len];
    let src_head = &src[..len];
    let g = _mm_set1_ps(gain);
    let mut i = 0usize;
    while i + 4 <= dst_head.len() {
        let s = _mm_loadu_ps(src_head.as_ptr().add(i));
        let r = _mm_mul_ps(s, g);
        _mm_storeu_ps(dst_head.as_mut_ptr().add(i), r);
        i += 4;
    }
    for (d, s) in dst_head[i..].iter_mut().zip(src_head[i..].iter()) {
        *d = *s * gain;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn affine_inplace_sse(dst: &mut [f32], scale: &[f32], loc: &[f32]) {
    let len = dst.len().min(scale.len()).min(loc.len());
    let mut i = 0usize;
    while i + 4 <= len {
        let d = _mm_loadu_ps(dst.as_ptr().add(i));
        let s = _mm_loadu_ps(scale.as_ptr().add(i));
        let l = _mm_loadu_ps(loc.as_ptr().add(i));
        let r = _mm_add_ps(_mm_mul_ps(d, s), l);
        _mm_storeu_ps(dst.as_mut_ptr().add(i), r);
        i += 4;
    }
    for ((d, s), l) in dst[i..len]
        .iter_mut()
        .zip(scale[i..len].iter())
        .zip(loc[i..len].iter())
    {
        *d = *d * *s + *l;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn dot_product_sse(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let mut sum = _mm_setzero_ps();
    let mut i = 0usize;
    while i + 4 <= len {
        let av = _mm_loadu_ps(a.as_ptr().add(i));
        let bv = _mm_loadu_ps(b.as_ptr().add(i));
        sum = _mm_add_ps(sum, _mm_mul_ps(av, bv));
        i += 4;
    }
    let mut arr = [0.0f32; 4];
    _mm_storeu_ps(arr.as_mut_ptr(), sum);
    let mut scalar = arr.into_iter().sum::<f32>();
    for j in i..len {
        scalar += a[j] * b[j];
    }
    scalar
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn add_scaled_inplace_avx(dst: &mut [f32], src: &[f32], gain: f32) {
    let len = dst.len().min(src.len());
    let dst_head = &mut dst[..len];
    let src_head = &src[..len];
    let g = _mm256_set1_ps(gain);
    let mut i = 0;
    while i + 8 <= dst_head.len() {
        let d = _mm256_loadu_ps(dst_head.as_ptr().add(i));
        let s = _mm256_loadu_ps(src_head.as_ptr().add(i));
        let r = _mm256_add_ps(d, _mm256_mul_ps(s, g));
        _mm256_storeu_ps(dst_head.as_mut_ptr().add(i), r);
        i += 8;
    }
    for (d, s) in dst_head[i..].iter_mut().zip(src_head[i..].iter()) {
        *d += *s * gain;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx,fma")]
unsafe fn add_scaled_inplace_avx_fma(dst: &mut [f32], src: &[f32], gain: f32) {
    let len = dst.len().min(src.len());
    let dst_head = &mut dst[..len];
    let src_head = &src[..len];
    let g = _mm256_set1_ps(gain);
    let mut i = 0;
    while i + 8 <= dst_head.len() {
        let d = _mm256_loadu_ps(dst_head.as_ptr().add(i));
        let s = _mm256_loadu_ps(src_head.as_ptr().add(i));
        let r = _mm256_fmadd_ps(s, g, d);
        _mm256_storeu_ps(dst_head.as_mut_ptr().add(i), r);
        i += 8;
    }
    for (d, s) in dst_head[i..].iter_mut().zip(src_head[i..].iter()) {
        *d += *s * gain;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn copy_scaled_inplace_avx(dst: &mut [f32], src: &[f32], gain: f32) {
    let len = dst.len().min(src.len());
    let dst_head = &mut dst[..len];
    let src_head = &src[..len];
    let g = _mm256_set1_ps(gain);
    let mut i = 0;
    while i + 8 <= dst_head.len() {
        let s = _mm256_loadu_ps(src_head.as_ptr().add(i));
        let r = _mm256_mul_ps(s, g);
        _mm256_storeu_ps(dst_head.as_mut_ptr().add(i), r);
        i += 8;
    }
    for (d, s) in dst_head[i..].iter_mut().zip(src_head[i..].iter()) {
        *d = *s * gain;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn affine_inplace_avx(dst: &mut [f32], scale: &[f32], loc: &[f32]) {
    let len = dst.len().min(scale.len()).min(loc.len());
    let mut i = 0;
    while i + 8 <= len {
        let d = _mm256_loadu_ps(dst.as_ptr().add(i));
        let s = _mm256_loadu_ps(scale.as_ptr().add(i));
        let l = _mm256_loadu_ps(loc.as_ptr().add(i));
        let r = _mm256_add_ps(_mm256_mul_ps(d, s), l);
        _mm256_storeu_ps(dst.as_mut_ptr().add(i), r);
        i += 8;
    }
    for ((d, s), l) in dst[i..len]
        .iter_mut()
        .zip(scale[i..len].iter())
        .zip(loc[i..len].iter())
    {
        *d = *d * *s + *l;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx,fma")]
unsafe fn affine_inplace_avx_fma(dst: &mut [f32], scale: &[f32], loc: &[f32]) {
    let len = dst.len().min(scale.len()).min(loc.len());
    let mut i = 0;
    while i + 8 <= len {
        let d = _mm256_loadu_ps(dst.as_ptr().add(i));
        let s = _mm256_loadu_ps(scale.as_ptr().add(i));
        let l = _mm256_loadu_ps(loc.as_ptr().add(i));
        let r = _mm256_fmadd_ps(d, s, l);
        _mm256_storeu_ps(dst.as_mut_ptr().add(i), r);
        i += 8;
    }
    for ((d, s), l) in dst[i..len]
        .iter_mut()
        .zip(scale[i..len].iter())
        .zip(loc[i..len].iter())
    {
        *d = *d * *s + *l;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn dot_product_avx(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let mut sum = _mm256_setzero_ps();
    let mut i = 0;
    while i + 8 <= len {
        let av = _mm256_loadu_ps(a.as_ptr().add(i));
        let bv = _mm256_loadu_ps(b.as_ptr().add(i));
        sum = _mm256_add_ps(sum, _mm256_mul_ps(av, bv));
        i += 8;
    }
    let mut arr = [0.0f32; 8];
    _mm256_storeu_ps(arr.as_mut_ptr(), sum);
    let mut scalar = arr.iter().sum::<f32>();
    for j in i..len {
        scalar += a[j] * b[j];
    }
    scalar
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx,fma")]
unsafe fn dot_product_avx_fma(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let mut sum = _mm256_setzero_ps();
    let mut i = 0;
    while i + 8 <= len {
        let av = _mm256_loadu_ps(a.as_ptr().add(i));
        let bv = _mm256_loadu_ps(b.as_ptr().add(i));
        sum = _mm256_fmadd_ps(av, bv, sum);
        i += 8;
    }
    let mut arr = [0.0f32; 8];
    _mm256_storeu_ps(arr.as_mut_ptr(), sum);
    let mut scalar = arr.iter().sum::<f32>();
    for j in i..len {
        scalar += a[j] * b[j];
    }
    scalar
}

/// dst[i] *= gain
pub fn mul_inplace(dst: &mut [f32], gain: f32) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            mul_inplace_avx(dst, gain);
            return;
        }
        if is_x86_feature_detected!("sse") {
            mul_inplace_sse(dst, gain);
            return;
        }
    }
    mul_inplace_scalar(dst, gain);
}

/// Replace NaN / ±Inf with 0.0 in place.
pub fn sanitize_finite_inplace(buf: &mut [f32]) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            sanitize_finite_inplace_avx(buf);
            return;
        }
        if is_x86_feature_detected!("sse") {
            sanitize_finite_inplace_sse(buf);
            return;
        }
    }
    sanitize_finite_inplace_scalar(buf);
}

fn mul_inplace_scalar(dst: &mut [f32], gain: f32) {
    for d in dst.iter_mut() {
        *d *= gain;
    }
}

fn sanitize_finite_inplace_scalar(buf: &mut [f32]) {
    for s in buf.iter_mut() {
        if !s.is_finite() {
            *s = 0.0;
        }
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn mul_inplace_sse(dst: &mut [f32], gain: f32) {
    let g = _mm_set1_ps(gain);
    let mut i = 0usize;
    while i + 4 <= dst.len() {
        let d = _mm_loadu_ps(dst.as_ptr().add(i));
        let r = _mm_mul_ps(d, g);
        _mm_storeu_ps(dst.as_mut_ptr().add(i), r);
        i += 4;
    }
    for d in &mut dst[i..] {
        *d *= gain;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn sanitize_finite_inplace_sse(buf: &mut [f32]) {
    let sign_mask = _mm_set1_ps(-0.0);
    let finite_max = _mm_set1_ps(f32::MAX);
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        let v = _mm_loadu_ps(buf.as_ptr().add(i));
        let abs_v = _mm_andnot_ps(sign_mask, v);
        let finite_mask = _mm_cmple_ps(abs_v, finite_max);
        let r = _mm_and_ps(v, finite_mask);
        _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 4;
    }
    for s in &mut buf[i..] {
        if !s.is_finite() {
            *s = 0.0;
        }
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn mul_inplace_avx(dst: &mut [f32], gain: f32) {
    let g = _mm256_set1_ps(gain);
    let mut i = 0;
    while i + 8 <= dst.len() {
        let d = _mm256_loadu_ps(dst.as_ptr().add(i));
        let r = _mm256_mul_ps(d, g);
        _mm256_storeu_ps(dst.as_mut_ptr().add(i), r);
        i += 8;
    }
    for d in &mut dst[i..] {
        *d *= gain;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn sanitize_finite_inplace_avx(buf: &mut [f32]) {
    let zero = _mm256_setzero_ps();
    let max_val = _mm256_set1_ps(f32::MAX);
    let mut i = 0;
    while i + 8 <= buf.len() {
        let v = _mm256_loadu_ps(buf.as_ptr().add(i));
        let abs_v = _mm256_andnot_ps(_mm256_set1_ps(-0.0), v);
        let mask = _mm256_cmp_ps(abs_v, max_val, _CMP_LE_OQ);
        let r = _mm256_blendv_ps(zero, v, mask);
        _mm256_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 8;
    }
    for s in &mut buf[i..] {
        if !s.is_finite() {
            *s = 0.0;
        }
    }
}

// ---------------------------------------------------------------------------
// Ramp-scaled copy / add — dst[i] = src[i] * ramp(i)  or  dst[i] += src[i] * ramp(i)
// ramp(i) = start_gain + i * delta,  delta = (end_gain - start_gain) / (len - 1)
// ---------------------------------------------------------------------------

/// dst[i] = src[i] * ramp(i)
pub fn copy_ramp_scaled_inplace(dst: &mut [f32], src: &[f32], start_gain: f32, end_gain: f32) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            copy_ramp_scaled_inplace_avx(dst, src, start_gain, end_gain);
            return;
        }
        if is_x86_feature_detected!("sse") {
            copy_ramp_scaled_inplace_sse(dst, src, start_gain, end_gain);
            return;
        }
    }
    copy_ramp_scaled_inplace_scalar(dst, src, start_gain, end_gain);
}

/// dst[i] += src[i] * ramp(i)
pub fn add_ramp_scaled_inplace(dst: &mut [f32], src: &[f32], start_gain: f32, end_gain: f32) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") && is_x86_feature_detected!("fma") {
            add_ramp_scaled_inplace_avx_fma(dst, src, start_gain, end_gain);
            return;
        }
        if is_x86_feature_detected!("avx") {
            add_ramp_scaled_inplace_avx(dst, src, start_gain, end_gain);
            return;
        }
        if is_x86_feature_detected!("sse") {
            add_ramp_scaled_inplace_sse(dst, src, start_gain, end_gain);
            return;
        }
    }
    add_ramp_scaled_inplace_scalar(dst, src, start_gain, end_gain);
}

fn copy_ramp_scaled_inplace_scalar(dst: &mut [f32], src: &[f32], start_gain: f32, end_gain: f32) {
    let len = dst.len().min(src.len());
    if len == 0 {
        return;
    }
    let delta = if len > 1 {
        (end_gain - start_gain) / (len - 1) as f32
    } else {
        0.0
    };
    for i in 0..len {
        dst[i] = src[i] * (start_gain + i as f32 * delta);
    }
}

fn add_ramp_scaled_inplace_scalar(dst: &mut [f32], src: &[f32], start_gain: f32, end_gain: f32) {
    let len = dst.len().min(src.len());
    if len == 0 {
        return;
    }
    let delta = if len > 1 {
        (end_gain - start_gain) / (len - 1) as f32
    } else {
        0.0
    };
    for i in 0..len {
        dst[i] += src[i] * (start_gain + i as f32 * delta);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn copy_ramp_scaled_inplace_sse(
    dst: &mut [f32],
    src: &[f32],
    start_gain: f32,
    end_gain: f32,
) {
    let len = dst.len().min(src.len());
    if len == 0 {
        return;
    }
    let delta = if len > 1 {
        (end_gain - start_gain) / (len - 1) as f32
    } else {
        0.0
    };
    let delta4 = _mm_set_ps(3.0 * delta, 2.0 * delta, 1.0 * delta, 0.0 * delta);
    let stride4 = _mm_set1_ps(4.0 * delta);
    let mut gain_vec = _mm_add_ps(_mm_set1_ps(start_gain), delta4);
    let mut i = 0usize;
    while i + 4 <= len {
        let s = _mm_loadu_ps(src.as_ptr().add(i));
        let r = _mm_mul_ps(s, gain_vec);
        _mm_storeu_ps(dst.as_mut_ptr().add(i), r);
        gain_vec = _mm_add_ps(gain_vec, stride4);
        i += 4;
    }
    for j in i..len {
        dst[j] = src[j] * (start_gain + j as f32 * delta);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn add_ramp_scaled_inplace_sse(
    dst: &mut [f32],
    src: &[f32],
    start_gain: f32,
    end_gain: f32,
) {
    let len = dst.len().min(src.len());
    if len == 0 {
        return;
    }
    let delta = if len > 1 {
        (end_gain - start_gain) / (len - 1) as f32
    } else {
        0.0
    };
    let delta4 = _mm_set_ps(3.0 * delta, 2.0 * delta, 1.0 * delta, 0.0 * delta);
    let stride4 = _mm_set1_ps(4.0 * delta);
    let mut gain_vec = _mm_add_ps(_mm_set1_ps(start_gain), delta4);
    let mut i = 0usize;
    while i + 4 <= len {
        let d = _mm_loadu_ps(dst.as_ptr().add(i));
        let s = _mm_loadu_ps(src.as_ptr().add(i));
        let r = _mm_add_ps(d, _mm_mul_ps(s, gain_vec));
        _mm_storeu_ps(dst.as_mut_ptr().add(i), r);
        gain_vec = _mm_add_ps(gain_vec, stride4);
        i += 4;
    }
    for j in i..len {
        dst[j] += src[j] * (start_gain + j as f32 * delta);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn copy_ramp_scaled_inplace_avx(
    dst: &mut [f32],
    src: &[f32],
    start_gain: f32,
    end_gain: f32,
) {
    let len = dst.len().min(src.len());
    if len == 0 {
        return;
    }
    let delta = if len > 1 {
        (end_gain - start_gain) / (len - 1) as f32
    } else {
        0.0
    };
    let delta8 = _mm256_set_ps(
        7.0 * delta,
        6.0 * delta,
        5.0 * delta,
        4.0 * delta,
        3.0 * delta,
        2.0 * delta,
        1.0 * delta,
        0.0 * delta,
    );
    let stride8 = _mm256_set1_ps(8.0 * delta);
    let mut gain_vec = _mm256_add_ps(_mm256_set1_ps(start_gain), delta8);
    let mut i = 0;
    while i + 8 <= len {
        let s = _mm256_loadu_ps(src.as_ptr().add(i));
        let r = _mm256_mul_ps(s, gain_vec);
        _mm256_storeu_ps(dst.as_mut_ptr().add(i), r);
        gain_vec = _mm256_add_ps(gain_vec, stride8);
        i += 8;
    }
    for j in i..len {
        dst[j] = src[j] * (start_gain + j as f32 * delta);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn add_ramp_scaled_inplace_avx(
    dst: &mut [f32],
    src: &[f32],
    start_gain: f32,
    end_gain: f32,
) {
    let len = dst.len().min(src.len());
    if len == 0 {
        return;
    }
    let delta = if len > 1 {
        (end_gain - start_gain) / (len - 1) as f32
    } else {
        0.0
    };
    let delta8 = _mm256_set_ps(
        7.0 * delta,
        6.0 * delta,
        5.0 * delta,
        4.0 * delta,
        3.0 * delta,
        2.0 * delta,
        1.0 * delta,
        0.0 * delta,
    );
    let stride8 = _mm256_set1_ps(8.0 * delta);
    let mut gain_vec = _mm256_add_ps(_mm256_set1_ps(start_gain), delta8);
    let mut i = 0;
    while i + 8 <= len {
        let d = _mm256_loadu_ps(dst.as_ptr().add(i));
        let s = _mm256_loadu_ps(src.as_ptr().add(i));
        let r = _mm256_add_ps(d, _mm256_mul_ps(s, gain_vec));
        _mm256_storeu_ps(dst.as_mut_ptr().add(i), r);
        gain_vec = _mm256_add_ps(gain_vec, stride8);
        i += 8;
    }
    for j in i..len {
        dst[j] += src[j] * (start_gain + j as f32 * delta);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx,fma")]
unsafe fn add_ramp_scaled_inplace_avx_fma(
    dst: &mut [f32],
    src: &[f32],
    start_gain: f32,
    end_gain: f32,
) {
    let len = dst.len().min(src.len());
    if len == 0 {
        return;
    }
    let delta = if len > 1 {
        (end_gain - start_gain) / (len - 1) as f32
    } else {
        0.0
    };
    let delta8 = _mm256_set_ps(
        7.0 * delta,
        6.0 * delta,
        5.0 * delta,
        4.0 * delta,
        3.0 * delta,
        2.0 * delta,
        1.0 * delta,
        0.0 * delta,
    );
    let stride8 = _mm256_set1_ps(8.0 * delta);
    let mut gain_vec = _mm256_add_ps(_mm256_set1_ps(start_gain), delta8);
    let mut i = 0;
    while i + 8 <= len {
        let d = _mm256_loadu_ps(dst.as_ptr().add(i));
        let s = _mm256_loadu_ps(src.as_ptr().add(i));
        let r = _mm256_fmadd_ps(s, gain_vec, d);
        _mm256_storeu_ps(dst.as_mut_ptr().add(i), r);
        gain_vec = _mm256_add_ps(gain_vec, stride8);
        i += 8;
    }
    for j in i..len {
        dst[j] += src[j] * (start_gain + j as f32 * delta);
    }
}
