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

fn add_inplace_scalar(dst: &mut [f32], src: &[f32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d += *s;
    }
}

fn add_scaled_inplace_scalar(dst: &mut [f32], src: &[f32], gain: f32) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d += *s * gain;
    }
}

fn affine_inplace_scalar(dst: &mut [f32], scale: &[f32], loc: &[f32]) {
    for ((d, s), l) in dst.iter_mut().zip(scale.iter()).zip(loc.iter()) {
        *d = *d * *s + *l;
    }
}

fn mul_inplace_scalar(dst: &mut [f32], gain: f32) {
    for d in dst.iter_mut() {
        *d *= gain;
    }
}

fn mul_per_sample_inplace_scalar(dst: &mut [f32], src: &[f32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d *= *s;
    }
}

fn dot_product_scalar(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn copy_scaled_inplace_scalar(dst: &mut [f32], src: &[f32], gain: f32) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = *s * gain;
    }
}

fn sanitize_finite_inplace_scalar(buf: &mut [f32]) {
    for s in buf.iter_mut() {
        if !s.is_finite() {
            *s = 0.0;
        }
    }
}

fn peak_abs_scalar(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0f32, |acc, s| acc.max(s.abs()))
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

// ---------------------------------------------------------------------------
// Activation SIMD helpers
// ---------------------------------------------------------------------------

/// Fast tanh approximation using the same coefficients as NAM C++.
#[inline]
pub fn fast_tanh(x: f32) -> f32 {
    let ax = x.abs();
    let x2 = x * x;
    let num = x * (2.455_507_5 + 2.455_507_5 * ax + (0.893_229_85 + 0.821_226_67 * ax) * x2);
    let den = 2.445_066_4 + (2.445_066_4 + x2) * (x + 0.814_642_7 * x * ax).abs();
    num / den
}

/// Fast sigmoid approximation via fast_tanh.
#[inline]
pub fn fast_sigmoid(x: f32) -> f32 {
    0.5 * (fast_tanh(x * 0.5) + 1.0)
}

/// Apply fast_tanh in-place with SIMD dispatch.
pub fn fast_tanh_inplace(buf: &mut [f32]) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            fast_tanh_inplace_avx(buf);
            return;
        }
        if is_x86_feature_detected!("sse") {
            fast_tanh_inplace_sse(buf);
            return;
        }
    }
    for x in buf {
        *x = fast_tanh(*x);
    }
}

/// Apply fast_sigmoid in-place with SIMD dispatch.
pub fn fast_sigmoid_inplace(buf: &mut [f32]) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            fast_sigmoid_inplace_avx(buf);
            return;
        }
        if is_x86_feature_detected!("sse") {
            fast_sigmoid_inplace_sse(buf);
            return;
        }
    }
    for x in buf {
        *x = fast_sigmoid(*x);
    }
}

/// ReLU in-place with SIMD dispatch.
pub fn relu_inplace(buf: &mut [f32]) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            relu_inplace_avx(buf);
            return;
        }
        if is_x86_feature_detected!("sse") {
            relu_inplace_sse(buf);
            return;
        }
    }
    for x in buf {
        if *x < 0.0 {
            *x = 0.0;
        }
    }
}

/// Hard tanh in-place with SIMD dispatch.
pub fn hardtanh_inplace(buf: &mut [f32]) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            hardtanh_inplace_avx(buf);
            return;
        }
        if is_x86_feature_detected!("sse") {
            hardtanh_inplace_sse(buf);
            return;
        }
    }
    for x in buf {
        *x = x.clamp(-1.0, 1.0);
    }
}

/// Leaky ReLU in-place with SIMD dispatch.
pub fn leaky_relu_inplace(buf: &mut [f32], negative_slope: f32) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            leaky_relu_inplace_avx(buf, negative_slope);
            return;
        }
        if is_x86_feature_detected!("sse") {
            leaky_relu_inplace_sse(buf, negative_slope);
            return;
        }
    }
    for x in buf {
        if *x < 0.0 {
            *x *= negative_slope;
        }
    }
}

/// dst[i] += fast_sigmoid(src[i]) * mul_src[i]
pub fn sigmoid_mul_add_inplace(dst: &mut [f32], src: &[f32], mul_src: &[f32]) {
    let n = dst.len().min(src.len()).min(mul_src.len());
    if n == 0 {
        return;
    }
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            sigmoid_mul_add_inplace_avx(&mut dst[..n], &src[..n], &mul_src[..n]);
            return;
        }
        if is_x86_feature_detected!("sse") {
            sigmoid_mul_add_inplace_sse(&mut dst[..n], &src[..n], &mul_src[..n]);
            return;
        }
    }
    for j in 0..n {
        dst[j] += fast_sigmoid(src[j]) * mul_src[j];
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
unsafe fn fast_tanh_m128(x: __m128) -> __m128 {
    let sign_mask = _mm_set1_ps(-0.0);
    let ax = _mm_andnot_ps(sign_mask, x);
    let x2 = _mm_mul_ps(x, x);
    let c1 = _mm_set1_ps(2.455_507_5);
    let c2 = _mm_set1_ps(0.893_229_85);
    let c3 = _mm_set1_ps(0.821_226_67);
    let c4 = _mm_set1_ps(2.445_066_4);
    let c5 = _mm_set1_ps(0.814_642_7);
    let num_inner = _mm_add_ps(c1, _mm_mul_ps(c1, ax));
    let num_tail = _mm_mul_ps(_mm_add_ps(c2, _mm_mul_ps(c3, ax)), x2);
    let num = _mm_mul_ps(x, _mm_add_ps(num_inner, num_tail));
    let den_inner = _mm_add_ps(x, _mm_mul_ps(_mm_mul_ps(c5, x), ax));
    let den_inner_abs = _mm_andnot_ps(sign_mask, den_inner);
    let den = _mm_add_ps(c4, _mm_mul_ps(_mm_add_ps(c4, x2), den_inner_abs));
    _mm_div_ps(num, den)
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
unsafe fn fast_sigmoid_m128(x: __m128) -> __m128 {
    let half = _mm_set1_ps(0.5);
    let one = _mm_set1_ps(1.0);
    _mm_mul_ps(half, _mm_add_ps(fast_tanh_m128(_mm_mul_ps(x, half)), one))
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
#[target_feature(enable = "avx")]
unsafe fn fast_tanh_m256(x: __m256) -> __m256 {
    let sign_mask = _mm256_set1_ps(-0.0);
    let ax = _mm256_andnot_ps(sign_mask, x);
    let x2 = _mm256_mul_ps(x, x);
    let c1 = _mm256_set1_ps(2.455_507_5);
    let c2 = _mm256_set1_ps(0.893_229_85);
    let c3 = _mm256_set1_ps(0.821_226_67);
    let c4 = _mm256_set1_ps(2.445_066_4);
    let c5 = _mm256_set1_ps(0.814_642_7);
    let num_inner = _mm256_add_ps(c1, _mm256_mul_ps(c1, ax));
    let num_tail = _mm256_mul_ps(_mm256_add_ps(c2, _mm256_mul_ps(c3, ax)), x2);
    let num = _mm256_mul_ps(x, _mm256_add_ps(num_inner, num_tail));
    let den_inner = _mm256_add_ps(x, _mm256_mul_ps(_mm256_mul_ps(c5, x), ax));
    let den_inner_abs = _mm256_andnot_ps(sign_mask, den_inner);
    let den = _mm256_add_ps(c4, _mm256_mul_ps(_mm256_add_ps(c4, x2), den_inner_abs));
    _mm256_div_ps(num, den)
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
#[target_feature(enable = "avx")]
unsafe fn fast_sigmoid_m256(x: __m256) -> __m256 {
    let half = _mm256_set1_ps(0.5);
    let one = _mm256_set1_ps(1.0);
    _mm256_mul_ps(
        half,
        _mm256_add_ps(fast_tanh_m256(_mm256_mul_ps(x, half)), one),
    )
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn fast_tanh_inplace_sse(buf: &mut [f32]) {
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        let x = _mm_loadu_ps(buf.as_ptr().add(i));
        let r = fast_tanh_m128(x);
        _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 4;
    }
    for x in &mut buf[i..] {
        *x = fast_tanh(*x);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn fast_sigmoid_inplace_sse(buf: &mut [f32]) {
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        let x = _mm_loadu_ps(buf.as_ptr().add(i));
        let r = fast_sigmoid_m128(x);
        _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 4;
    }
    for x in &mut buf[i..] {
        *x = fast_sigmoid(*x);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn relu_inplace_sse(buf: &mut [f32]) {
    let zero = _mm_setzero_ps();
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        let x = _mm_loadu_ps(buf.as_ptr().add(i));
        let r = _mm_max_ps(x, zero);
        _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 4;
    }
    for x in &mut buf[i..] {
        if *x < 0.0 {
            *x = 0.0;
        }
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn hardtanh_inplace_sse(buf: &mut [f32]) {
    let neg = _mm_set1_ps(-1.0);
    let pos = _mm_set1_ps(1.0);
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        let x = _mm_loadu_ps(buf.as_ptr().add(i));
        let r = _mm_min_ps(_mm_max_ps(x, neg), pos);
        _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 4;
    }
    for x in &mut buf[i..] {
        *x = x.clamp(-1.0, 1.0);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn leaky_relu_inplace_sse(buf: &mut [f32], negative_slope: f32) {
    let zero = _mm_setzero_ps();
    let slope = _mm_set1_ps(negative_slope);
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        let x = _mm_loadu_ps(buf.as_ptr().add(i));
        let scaled = _mm_mul_ps(x, slope);
        let mask = _mm_cmpgt_ps(x, zero);
        let r = _mm_or_ps(_mm_and_ps(mask, x), _mm_andnot_ps(mask, scaled));
        _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 4;
    }
    for x in &mut buf[i..] {
        if *x < 0.0 {
            *x *= negative_slope;
        }
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn sigmoid_mul_add_inplace_sse(dst: &mut [f32], src: &[f32], mul_src: &[f32]) {
    let n = dst.len().min(src.len()).min(mul_src.len());
    let mut i = 0usize;
    while i + 4 <= n {
        let s = _mm_loadu_ps(src.as_ptr().add(i));
        let m = _mm_loadu_ps(mul_src.as_ptr().add(i));
        let d = _mm_loadu_ps(dst.as_ptr().add(i));
        let sig = fast_sigmoid_m128(s);
        let r = _mm_add_ps(d, _mm_mul_ps(sig, m));
        _mm_storeu_ps(dst.as_mut_ptr().add(i), r);
        i += 4;
    }
    for j in i..n {
        dst[j] += fast_sigmoid(src[j]) * mul_src[j];
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn fast_tanh_inplace_avx(buf: &mut [f32]) {
    let mut i = 0usize;
    while i + 8 <= buf.len() {
        let x = _mm256_loadu_ps(buf.as_ptr().add(i));
        let r = fast_tanh_m256(x);
        _mm256_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 8;
    }
    if i + 4 <= buf.len() {
        let x = _mm_loadu_ps(buf.as_ptr().add(i));
        let r = fast_tanh_m128(x);
        _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 4;
    }
    for x in &mut buf[i..] {
        *x = fast_tanh(*x);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn fast_sigmoid_inplace_avx(buf: &mut [f32]) {
    let mut i = 0usize;
    while i + 8 <= buf.len() {
        let x = _mm256_loadu_ps(buf.as_ptr().add(i));
        let r = fast_sigmoid_m256(x);
        _mm256_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 8;
    }
    if i + 4 <= buf.len() {
        let x = _mm_loadu_ps(buf.as_ptr().add(i));
        let r = fast_sigmoid_m128(x);
        _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 4;
    }
    for x in &mut buf[i..] {
        *x = fast_sigmoid(*x);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn relu_inplace_avx(buf: &mut [f32]) {
    let zero = _mm256_setzero_ps();
    let mut i = 0usize;
    while i + 8 <= buf.len() {
        let x = _mm256_loadu_ps(buf.as_ptr().add(i));
        let r = _mm256_max_ps(x, zero);
        _mm256_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 8;
    }
    for x in &mut buf[i..] {
        if *x < 0.0 {
            *x = 0.0;
        }
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn hardtanh_inplace_avx(buf: &mut [f32]) {
    let neg = _mm256_set1_ps(-1.0);
    let pos = _mm256_set1_ps(1.0);
    let mut i = 0usize;
    while i + 8 <= buf.len() {
        let x = _mm256_loadu_ps(buf.as_ptr().add(i));
        let r = _mm256_min_ps(_mm256_max_ps(x, neg), pos);
        _mm256_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 8;
    }
    for x in &mut buf[i..] {
        *x = x.clamp(-1.0, 1.0);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn leaky_relu_inplace_avx(buf: &mut [f32], negative_slope: f32) {
    let zero = _mm256_setzero_ps();
    let slope = _mm256_set1_ps(negative_slope);
    let mut i = 0usize;
    while i + 8 <= buf.len() {
        let x = _mm256_loadu_ps(buf.as_ptr().add(i));
        let scaled = _mm256_mul_ps(x, slope);
        let mask = _mm256_cmp_ps(x, zero, _CMP_GT_OQ);
        let r = _mm256_blendv_ps(scaled, x, mask);
        _mm256_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 8;
    }
    for x in &mut buf[i..] {
        if *x < 0.0 {
            *x *= negative_slope;
        }
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn sigmoid_mul_add_inplace_avx(dst: &mut [f32], src: &[f32], mul_src: &[f32]) {
    let n = dst.len().min(src.len()).min(mul_src.len());
    let mut i = 0usize;
    while i + 8 <= n {
        let s = _mm256_loadu_ps(src.as_ptr().add(i));
        let m = _mm256_loadu_ps(mul_src.as_ptr().add(i));
        let d = _mm256_loadu_ps(dst.as_ptr().add(i));
        let sig = fast_sigmoid_m256(s);
        let r = _mm256_add_ps(d, _mm256_mul_ps(sig, m));
        _mm256_storeu_ps(dst.as_mut_ptr().add(i), r);
        i += 8;
    }
    if i + 4 <= n {
        let s = _mm_loadu_ps(src.as_ptr().add(i));
        let m = _mm_loadu_ps(mul_src.as_ptr().add(i));
        let d = _mm_loadu_ps(dst.as_ptr().add(i));
        let sig = fast_sigmoid_m128(s);
        let r = _mm_add_ps(d, _mm_mul_ps(sig, m));
        _mm_storeu_ps(dst.as_mut_ptr().add(i), r);
        i += 4;
    }
    for j in i..n {
        dst[j] += fast_sigmoid(src[j]) * mul_src[j];
    }
}

/// Softsign in-place with SIMD dispatch.
pub fn softsign_inplace(buf: &mut [f32]) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            softsign_inplace_avx(buf);
            return;
        }
        if is_x86_feature_detected!("sse") {
            softsign_inplace_sse(buf);
            return;
        }
    }
    for x in buf {
        *x /= 1.0 + x.abs();
    }
}

/// Hardswish in-place with SIMD dispatch.
pub fn hardswish_inplace(buf: &mut [f32]) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        if is_x86_feature_detected!("avx") {
            hardswish_inplace_avx(buf);
            return;
        }
        if is_x86_feature_detected!("sse") {
            hardswish_inplace_sse(buf);
            return;
        }
    }
    for x in buf {
        let t = *x + 3.0;
        let clamped = t.clamp(0.0, 6.0);
        *x = *x * clamped * (1.0 / 6.0);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn softsign_inplace_sse(buf: &mut [f32]) {
    let one = _mm_set1_ps(1.0);
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        let x = _mm_loadu_ps(buf.as_ptr().add(i));
        let abs_x = _mm_andnot_ps(_mm_set1_ps(-0.0), x);
        let den = _mm_add_ps(one, abs_x);
        let r = _mm_div_ps(x, den);
        _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 4;
    }
    for x in &mut buf[i..] {
        *x /= 1.0 + x.abs();
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse")]
unsafe fn hardswish_inplace_sse(buf: &mut [f32]) {
    let three = _mm_set1_ps(3.0);
    let zero = _mm_setzero_ps();
    let six = _mm_set1_ps(6.0);
    let inv6 = _mm_set1_ps(1.0 / 6.0);
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        let x = _mm_loadu_ps(buf.as_ptr().add(i));
        let t = _mm_add_ps(x, three);
        let clamped = _mm_min_ps(_mm_max_ps(t, zero), six);
        let r = _mm_mul_ps(x, _mm_mul_ps(clamped, inv6));
        _mm_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 4;
    }
    for x in &mut buf[i..] {
        let t = *x + 3.0;
        let clamped = t.clamp(0.0, 6.0);
        *x = *x * clamped * (1.0 / 6.0);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn softsign_inplace_avx(buf: &mut [f32]) {
    let one = _mm256_set1_ps(1.0);
    let mut i = 0usize;
    while i + 8 <= buf.len() {
        let x = _mm256_loadu_ps(buf.as_ptr().add(i));
        let abs_x = _mm256_andnot_ps(_mm256_set1_ps(-0.0), x);
        let den = _mm256_add_ps(one, abs_x);
        let r = _mm256_div_ps(x, den);
        _mm256_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 8;
    }
    for x in &mut buf[i..] {
        *x /= 1.0 + x.abs();
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx")]
unsafe fn hardswish_inplace_avx(buf: &mut [f32]) {
    let three = _mm256_set1_ps(3.0);
    let zero = _mm256_setzero_ps();
    let six = _mm256_set1_ps(6.0);
    let inv6 = _mm256_set1_ps(1.0 / 6.0);
    let mut i = 0usize;
    while i + 8 <= buf.len() {
        let x = _mm256_loadu_ps(buf.as_ptr().add(i));
        let t = _mm256_add_ps(x, three);
        let clamped = _mm256_min_ps(_mm256_max_ps(t, zero), six);
        let r = _mm256_mul_ps(x, _mm256_mul_ps(clamped, inv6));
        _mm256_storeu_ps(buf.as_mut_ptr().add(i), r);
        i += 8;
    }
    for x in &mut buf[i..] {
        let t = *x + 3.0;
        let clamped = t.clamp(0.0, 6.0);
        *x = *x * clamped * (1.0 / 6.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_inplace_basic() {
        let mut a = [1.0f32, 2.0, 3.0, 4.0, 5.0];
        let b = [10.0f32, 20.0, 30.0, 40.0, 50.0];
        add_inplace(&mut a, &b);
        assert_eq!(a, [11.0, 22.0, 33.0, 44.0, 55.0]);
    }

    #[test]
    fn mul_inplace_basic() {
        let mut a = [1.0f32, 2.0, 3.0, 4.0, 5.0];
        mul_inplace(&mut a, 2.0);
        assert_eq!(a, [2.0, 4.0, 6.0, 8.0, 10.0]);
    }

    #[test]
    fn add_scaled_inplace_basic() {
        let mut a = [1.0f32, 2.0, 3.0, 4.0, 5.0];
        let b = [10.0f32, 20.0, 30.0, 40.0, 50.0];
        add_scaled_inplace(&mut a, &b, 0.5);
        assert_eq!(a, [6.0, 12.0, 18.0, 24.0, 30.0]);
    }

    #[test]
    fn copy_scaled_inplace_basic() {
        let mut a = [0.0f32; 5];
        let b = [10.0f32, 20.0, 30.0, 40.0, 50.0];
        copy_scaled_inplace(&mut a, &b, 0.5);
        assert_eq!(a, [5.0, 10.0, 15.0, 20.0, 25.0]);
    }

    #[test]
    fn affine_inplace_basic() {
        let mut a = [1.0f32, 2.0, 3.0, 4.0];
        let scale = [2.0f32, 2.0, 2.0, 2.0];
        let loc = [1.0f32, 1.0, 1.0, 1.0];
        affine_inplace(&mut a, &scale, &loc);
        assert_eq!(a, [3.0, 5.0, 7.0, 9.0]);
    }

    #[test]
    fn mul_per_sample_inplace_basic() {
        let mut a = [1.0f32, 2.0, 3.0, 4.0, 5.0];
        let b = [2.0f32, 3.0, 4.0, 5.0, 6.0];
        mul_per_sample_inplace(&mut a, &b);
        assert_eq!(a, [2.0, 6.0, 12.0, 20.0, 30.0]);
    }

    #[test]
    fn dot_product_basic() {
        let a = [1.0f32, 2.0, 3.0, 4.0];
        let b = [0.5f32, 1.0, 1.5, 2.0];
        assert_eq!(dot_product(&a, &b), 15.0);
    }

    #[test]
    fn peak_abs_basic() {
        let a = [1.0f32, -3.0, 2.0, 0.5];
        assert_eq!(peak_abs(&a), 3.0);
    }

    #[test]
    fn sanitize_finite_inplace_basic() {
        let mut a = [1.0f32, f32::NAN, f32::INFINITY, 4.0, f32::NEG_INFINITY];
        sanitize_finite_inplace(&mut a);
        assert!(a[0].is_finite() && a[0] == 1.0);
        assert_eq!(a[1], 0.0);
        assert_eq!(a[2], 0.0);
        assert!(a[3].is_finite() && a[3] == 4.0);
        assert_eq!(a[4], 0.0);
    }

    #[test]
    fn copy_ramp_scaled_inplace_basic() {
        let mut dst = [0.0f32; 5];
        let src = [1.0f32, 1.0, 1.0, 1.0, 1.0];
        copy_ramp_scaled_inplace(&mut dst, &src, 0.0, 1.0);
        assert_eq!(dst, [0.0, 0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn add_ramp_scaled_inplace_basic() {
        let mut dst = [1.0f32, 1.0, 1.0, 1.0, 1.0];
        let src = [1.0f32, 1.0, 1.0, 1.0, 1.0];
        add_ramp_scaled_inplace(&mut dst, &src, 0.0, 1.0);
        assert_eq!(dst, [1.0, 1.25, 1.5, 1.75, 2.0]);
    }

    #[test]
    fn fast_tanh_inplace_basic() {
        let mut a = [0.0f32, 1.0, -1.0, 2.0, -2.0];
        fast_tanh_inplace(&mut a);
        assert!(a[0].abs() < 1e-6);
        assert!((a[1] - fast_tanh(1.0)).abs() < 1e-6);
        assert!((a[2] - fast_tanh(-1.0)).abs() < 1e-6);
    }

    #[test]
    fn fast_sigmoid_inplace_basic() {
        let mut a = [0.0f32, 1.0, -1.0];
        fast_sigmoid_inplace(&mut a);
        assert!((a[0] - 0.5).abs() < 1e-5);
        assert!((a[1] - fast_sigmoid(1.0)).abs() < 1e-5);
        assert!((a[2] - fast_sigmoid(-1.0)).abs() < 1e-5);
    }

    #[test]
    fn relu_inplace_basic() {
        let mut a = [-2.0f32, -0.5, 0.0, 0.5, 2.0];
        relu_inplace(&mut a);
        assert_eq!(a, [0.0, 0.0, 0.0, 0.5, 2.0]);
    }

    #[test]
    fn hardtanh_inplace_basic() {
        let mut a = [-2.0f32, -0.5, 0.0, 0.5, 2.0];
        hardtanh_inplace(&mut a);
        assert_eq!(a, [-1.0, -0.5, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn leaky_relu_inplace_basic() {
        let mut a = [-2.0f32, -0.5, 0.0, 0.5, 2.0];
        leaky_relu_inplace(&mut a, 0.1);
        assert_eq!(a, [-0.2, -0.05, 0.0, 0.5, 2.0]);
    }

    #[test]
    fn sigmoid_mul_add_inplace_basic() {
        let mut dst = [1.0f32, 1.0, 1.0, 1.0];
        let src = [0.0f32, 1.0, -1.0, 2.0];
        let mul = [1.0f32, 1.0, 1.0, 1.0];
        sigmoid_mul_add_inplace(&mut dst, &src, &mul);
        assert!((dst[0] - 1.5).abs() < 1e-5);
        assert!((dst[1] - (1.0 + fast_sigmoid(1.0))).abs() < 1e-5);
        assert!((dst[2] - (1.0 + fast_sigmoid(-1.0))).abs() < 1e-5);
    }

    #[test]
    fn softsign_inplace_basic() {
        let mut a = [0.0f32, 1.0, -1.0, 2.0, -2.0];
        softsign_inplace(&mut a);
        assert!(a[0].abs() < 1e-6);
        assert!((a[1] - 0.5).abs() < 1e-6);
        assert!((a[2] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn hardswish_inplace_basic() {
        let mut a = [-4.0f32, -3.0, 0.0, 3.0, 4.0];
        hardswish_inplace(&mut a);
        assert!(a[0].abs() < 1e-6);
        assert!(a[1].abs() < 1e-6);
        assert!(a[2].abs() < 1e-6);
        assert!((a[3] - 3.0).abs() < 1e-6);
        assert!((a[4] - 4.0).abs() < 1e-6);
    }
}
