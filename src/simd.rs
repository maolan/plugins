use fearless_simd::prelude::*;
use fearless_simd::{Level, dispatch};
use fearless_simd_macros::simd;

/// Enables flush-to-zero (FTZ) and denormals-are-zero (DAZ) for the calling
/// thread so recursive filter/delay states decay to exact zero instead of
/// crawling through denormals. Called once per audio block; on non-x86
/// targets it is a no-op (denormal protection then relies on the +1e-24 bias
/// in the recursive DSP structs).
pub fn enable_flush_to_zero() {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        if is_x86_feature_detected!("sse") {
            // FTZ (bit 15) and DAZ (bit 6) of MXCSR. `_mm_setcsr` is
            // deprecated in favor of inline assembly, which we use directly.
            const MXCSR_FTZ: u32 = 1 << 15;
            const MXCSR_DAZ: u32 = 1 << 6;
            unsafe {
                let mut csr: u32 = 0;
                std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr, options(nostack));
                csr |= MXCSR_FTZ | MXCSR_DAZ;
                std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(readonly, nostack));
            }
        }
    }
}

pub fn add_inplace(dst: &mut [f32], src: &[f32]) {
    dispatch!(Level::new(), simd => add_inplace_kernel(simd, dst, src));
}

pub fn add_scaled_inplace(dst: &mut [f32], src: &[f32], gain: f32) {
    dispatch!(Level::new(), simd => add_scaled_inplace_kernel(simd, dst, src, gain));
}

pub fn affine_inplace(dst: &mut [f32], scale: &[f32], loc: &[f32]) {
    dispatch!(Level::new(), simd => affine_inplace_kernel(simd, dst, scale, loc));
}

pub fn mul_inplace(dst: &mut [f32], gain: f32) {
    dispatch!(Level::new(), simd => mul_inplace_kernel(simd, dst, gain));
}

pub fn mul_per_sample_inplace(dst: &mut [f32], src: &[f32]) {
    dispatch!(Level::new(), simd => mul_per_sample_inplace_kernel(simd, dst, src));
}

pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    dispatch!(Level::new(), simd => dot_product_kernel(simd, a, b))
}

pub fn copy_scaled_inplace(dst: &mut [f32], src: &[f32], gain: f32) {
    dispatch!(Level::new(), simd => copy_scaled_inplace_kernel(simd, dst, src, gain));
}

pub fn sanitize_finite_inplace(buf: &mut [f32]) {
    dispatch!(Level::new(), simd => sanitize_finite_inplace_kernel(simd, buf));
}

pub fn peak_abs(buf: &[f32]) -> f32 {
    dispatch!(Level::new(), simd => peak_abs_kernel(simd, buf))
}

pub fn clip_inplace(buf: &mut [f32], limit: f32) {
    dispatch!(Level::new(), simd => clip_inplace_kernel(simd, buf, limit));
}

pub fn copy_ramp_scaled_inplace(dst: &mut [f32], src: &[f32], start_gain: f32, end_gain: f32) {
    dispatch!(Level::new(), simd => copy_ramp_scaled_inplace_kernel(simd, dst, src, start_gain, end_gain));
}

pub fn add_ramp_scaled_inplace(dst: &mut [f32], src: &[f32], start_gain: f32, end_gain: f32) {
    dispatch!(Level::new(), simd => add_ramp_scaled_inplace_kernel(simd, dst, src, start_gain, end_gain));
}

#[inline]
pub fn fast_tanh(x: f32) -> f32 {
    let ax = x.abs();
    let x2 = x * x;
    let num = x * (2.455_507_5 + 2.455_507_5 * ax + (0.893_229_85 + 0.821_226_67 * ax) * x2);
    let den = 2.445_066_4 + (2.445_066_4 + x2) * (x + 0.814_642_7 * x * ax).abs();
    num / den
}

#[inline]
pub fn fast_sigmoid(x: f32) -> f32 {
    0.5 * (fast_tanh(x * 0.5) + 1.0)
}

pub fn fast_tanh_inplace(buf: &mut [f32]) {
    dispatch!(Level::new(), simd => fast_tanh_inplace_kernel(simd, buf));
}

pub fn fast_sigmoid_inplace(buf: &mut [f32]) {
    dispatch!(Level::new(), simd => fast_sigmoid_inplace_kernel(simd, buf));
}

pub fn relu_inplace(buf: &mut [f32]) {
    dispatch!(Level::new(), simd => relu_inplace_kernel(simd, buf));
}

pub fn hardtanh_inplace(buf: &mut [f32]) {
    dispatch!(Level::new(), simd => hardtanh_inplace_kernel(simd, buf));
}

pub fn leaky_relu_inplace(buf: &mut [f32], negative_slope: f32) {
    dispatch!(Level::new(), simd => leaky_relu_inplace_kernel(simd, buf, negative_slope));
}

pub fn sigmoid_mul_add_inplace(dst: &mut [f32], src: &[f32], mul_src: &[f32]) {
    dispatch!(Level::new(), simd => sigmoid_mul_add_inplace_kernel(simd, dst, src, mul_src));
}

pub fn softsign_inplace(buf: &mut [f32]) {
    dispatch!(Level::new(), simd => softsign_inplace_kernel(simd, buf));
}

/// Tanh-based soft clipper: identity for `|x| <= knee`, saturating smoothly
/// toward ±1 above it. Bit-identical to applying the scalar per-sample
/// formula; the tanh evaluation itself stays scalar libm, the surrounding
/// arithmetic is vectorized.
pub fn soft_clip_inplace(buf: &mut [f32], knee: f32) {
    dispatch!(Level::new(), simd => soft_clip_inplace_kernel(simd, buf, knee));
}

pub fn hardswish_inplace(buf: &mut [f32]) {
    dispatch!(Level::new(), simd => hardswish_inplace_kernel(simd, buf));
}

#[simd]
fn add_inplace_kernel<S: Simd>(simd: S, dst: &mut [f32], src: &[f32]) {
    let n = dst.len().min(src.len());
    let dst = &mut dst[..n];
    let src = &src[..n];
    let mut dchunks = dst.chunks_exact_mut(S::f32s::LEN);
    let mut schunks = src.chunks_exact(S::f32s::LEN);
    for (d, s) in dchunks.by_ref().zip(schunks.by_ref()) {
        let dv = S::f32s::from_slice(simd, d);
        let sv = S::f32s::from_slice(simd, s);
        (dv + sv).store_slice(d);
    }
    for (d, s) in dchunks.into_remainder().iter_mut().zip(schunks.remainder()) {
        *d += *s;
    }
}

#[simd]
fn add_scaled_inplace_kernel<S: Simd>(simd: S, dst: &mut [f32], src: &[f32], gain: f32) {
    let n = dst.len().min(src.len());
    let dst = &mut dst[..n];
    let src = &src[..n];
    let mut dchunks = dst.chunks_exact_mut(S::f32s::LEN);
    let mut schunks = src.chunks_exact(S::f32s::LEN);
    for (d, s) in dchunks.by_ref().zip(schunks.by_ref()) {
        let dv = S::f32s::from_slice(simd, d);
        let sv = S::f32s::from_slice(simd, s);
        (dv + sv * gain).store_slice(d);
    }
    for (d, s) in dchunks.into_remainder().iter_mut().zip(schunks.remainder()) {
        *d += *s * gain;
    }
}

#[simd]
fn affine_inplace_kernel<S: Simd>(simd: S, dst: &mut [f32], scale: &[f32], loc: &[f32]) {
    let n = dst.len().min(scale.len()).min(loc.len());
    let dst = &mut dst[..n];
    let scale = &scale[..n];
    let loc = &loc[..n];
    let mut dchunks = dst.chunks_exact_mut(S::f32s::LEN);
    let mut schunks = scale.chunks_exact(S::f32s::LEN);
    let mut lchunks = loc.chunks_exact(S::f32s::LEN);
    for ((d, s), l) in dchunks.by_ref().zip(schunks.by_ref()).zip(lchunks.by_ref()) {
        let dv = S::f32s::from_slice(simd, d);
        let sv = S::f32s::from_slice(simd, s);
        let lv = S::f32s::from_slice(simd, l);
        (dv * sv + lv).store_slice(d);
    }
    for ((d, s), l) in dchunks
        .into_remainder()
        .iter_mut()
        .zip(schunks.remainder())
        .zip(lchunks.remainder())
    {
        *d = *d * *s + *l;
    }
}

#[simd]
fn mul_inplace_kernel<S: Simd>(simd: S, dst: &mut [f32], gain: f32) {
    let mut chunks = dst.chunks_exact_mut(S::f32s::LEN);
    for d in chunks.by_ref() {
        (S::f32s::from_slice(simd, d) * gain).store_slice(d);
    }
    for d in chunks.into_remainder() {
        *d *= gain;
    }
}

#[simd]
fn mul_per_sample_inplace_kernel<S: Simd>(simd: S, dst: &mut [f32], src: &[f32]) {
    let n = dst.len().min(src.len());
    let dst = &mut dst[..n];
    let src = &src[..n];
    let mut dchunks = dst.chunks_exact_mut(S::f32s::LEN);
    let mut schunks = src.chunks_exact(S::f32s::LEN);
    for (d, s) in dchunks.by_ref().zip(schunks.by_ref()) {
        let dv = S::f32s::from_slice(simd, d);
        let sv = S::f32s::from_slice(simd, s);
        (dv * sv).store_slice(d);
    }
    for (d, s) in dchunks.into_remainder().iter_mut().zip(schunks.remainder()) {
        *d *= *s;
    }
}

#[simd]
fn dot_product_kernel<S: Simd>(simd: S, a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let a = &a[..n];
    let b = &b[..n];
    let mut acc = S::f32s::splat(simd, 0.0);
    let mut achunks = a.chunks_exact(S::f32s::LEN);
    let mut bchunks = b.chunks_exact(S::f32s::LEN);
    for (ac, bc) in achunks.by_ref().zip(bchunks.by_ref()) {
        let av = S::f32s::from_slice(simd, ac);
        let bv = S::f32s::from_slice(simd, bc);
        acc += av * bv;
    }
    let mut sum = acc.reduce_sum();
    for (x, y) in achunks.remainder().iter().zip(bchunks.remainder()) {
        sum += x * y;
    }
    sum
}

#[simd]
fn copy_scaled_inplace_kernel<S: Simd>(simd: S, dst: &mut [f32], src: &[f32], gain: f32) {
    let n = dst.len().min(src.len());
    let dst = &mut dst[..n];
    let src = &src[..n];
    let mut dchunks = dst.chunks_exact_mut(S::f32s::LEN);
    let mut schunks = src.chunks_exact(S::f32s::LEN);
    for (d, s) in dchunks.by_ref().zip(schunks.by_ref()) {
        let sv = S::f32s::from_slice(simd, s);
        (sv * gain).store_slice(d);
    }
    for (d, s) in dchunks.into_remainder().iter_mut().zip(schunks.remainder()) {
        *d = *s * gain;
    }
}

#[simd]
fn sanitize_finite_inplace_kernel<S: Simd>(simd: S, buf: &mut [f32]) {
    let zero = S::f32s::splat(simd, 0.0);
    let mut chunks = buf.chunks_exact_mut(S::f32s::LEN);
    for b in chunks.by_ref() {
        let v = S::f32s::from_slice(simd, b);
        v.is_finite().select(v, zero).store_slice(b);
    }
    for s in chunks.into_remainder() {
        if !s.is_finite() {
            *s = 0.0;
        }
    }
}

#[simd]
fn peak_abs_kernel<S: Simd>(simd: S, buf: &[f32]) -> f32 {
    let mut acc = S::f32s::splat(simd, 0.0);
    let mut chunks = buf.chunks_exact(S::f32s::LEN);
    for c in chunks.by_ref() {
        acc = acc.max(S::f32s::from_slice(simd, c).abs());
    }
    let mut m = acc.reduce_max();
    for v in chunks.remainder() {
        m = m.max(v.abs());
    }
    m
}

#[simd]
fn clip_inplace_kernel<S: Simd>(simd: S, buf: &mut [f32], limit: f32) {
    let neg = S::f32s::splat(simd, -limit);
    let pos = S::f32s::splat(simd, limit);
    let mut chunks = buf.chunks_exact_mut(S::f32s::LEN);
    for b in chunks.by_ref() {
        let v = S::f32s::from_slice(simd, b);
        v.max(neg).min(pos).store_slice(b);
    }
    for s in chunks.into_remainder() {
        *s = s.clamp(-limit, limit);
    }
}

#[simd]
fn copy_ramp_scaled_inplace_kernel<S: Simd>(
    simd: S,
    dst: &mut [f32],
    src: &[f32],
    start_gain: f32,
    end_gain: f32,
) {
    let n = dst.len().min(src.len());
    if n == 0 {
        return;
    }
    let dst = &mut dst[..n];
    let src = &src[..n];
    let delta = if n > 1 {
        (end_gain - start_gain) / (n - 1) as f32
    } else {
        0.0
    };
    let len = S::f32s::LEN;
    let mut dchunks = dst.chunks_exact_mut(len);
    let mut schunks = src.chunks_exact(len);
    let mut base = 0usize;
    for (d, s) in dchunks.by_ref().zip(schunks.by_ref()) {
        // Per-lane gains match the scalar reference bit-for-bit (start + i*delta).
        let mut gains = [0.0f32; 16];
        for (lane, g) in gains.iter_mut().enumerate() {
            *g = start_gain + (base + lane) as f32 * delta;
        }
        let gv = S::f32s::from_slice(simd, &gains[..len]);
        (S::f32s::from_slice(simd, s) * gv).store_slice(d);
        base += len;
    }
    for (j, (d, s)) in dchunks
        .into_remainder()
        .iter_mut()
        .zip(schunks.remainder())
        .enumerate()
    {
        *d = *s * (start_gain + (base + j) as f32 * delta);
    }
}

#[simd]
fn add_ramp_scaled_inplace_kernel<S: Simd>(
    simd: S,
    dst: &mut [f32],
    src: &[f32],
    start_gain: f32,
    end_gain: f32,
) {
    let n = dst.len().min(src.len());
    if n == 0 {
        return;
    }
    let dst = &mut dst[..n];
    let src = &src[..n];
    let delta = if n > 1 {
        (end_gain - start_gain) / (n - 1) as f32
    } else {
        0.0
    };
    let len = S::f32s::LEN;
    let mut dchunks = dst.chunks_exact_mut(len);
    let mut schunks = src.chunks_exact(len);
    let mut base = 0usize;
    for (d, s) in dchunks.by_ref().zip(schunks.by_ref()) {
        let mut gains = [0.0f32; 16];
        for (lane, g) in gains.iter_mut().enumerate() {
            *g = start_gain + (base + lane) as f32 * delta;
        }
        let gv = S::f32s::from_slice(simd, &gains[..len]);
        let dv = S::f32s::from_slice(simd, d);
        (dv + S::f32s::from_slice(simd, s) * gv).store_slice(d);
        base += len;
    }
    for (j, (d, s)) in dchunks
        .into_remainder()
        .iter_mut()
        .zip(schunks.remainder())
        .enumerate()
    {
        *d += *s * (start_gain + (base + j) as f32 * delta);
    }
}

/// Vectorized `fast_tanh` with the same operation order as the scalar
/// version, so both produce bit-identical results per lane.
#[inline(always)]
fn fast_tanh_vec<S: Simd>(_simd: S, x: S::f32s) -> S::f32s {
    let ax = x.abs();
    let x2 = x * x;
    let c1 = 2.455_507_5;
    let c2 = 0.893_229_85;
    let c3 = 0.821_226_67;
    let c4 = 2.445_066_4;
    let c5 = 0.814_642_7;
    // Scalar on the left is not available for a generic vector type, so the
    // constants sit on the right; float add/mul are commutative, so the
    // results are bit-identical to the scalar `fast_tanh`.
    let num_inner = ax * c1 + c1;
    let num_tail = (ax * c3 + c2) * x2;
    let num = x * (num_inner + num_tail);
    let den_inner = x + (x * c5) * ax;
    let den = (x2 + c4) * den_inner.abs() + c4;
    num / den
}

#[inline(always)]
fn fast_sigmoid_vec<S: Simd>(simd: S, x: S::f32s) -> S::f32s {
    (fast_tanh_vec(simd, x * 0.5) + 1.0) * 0.5
}

#[simd]
fn fast_tanh_inplace_kernel<S: Simd>(simd: S, buf: &mut [f32]) {
    let mut chunks = buf.chunks_exact_mut(S::f32s::LEN);
    for b in chunks.by_ref() {
        let x = S::f32s::from_slice(simd, b);
        fast_tanh_vec(simd, x).store_slice(b);
    }
    for x in chunks.into_remainder() {
        *x = fast_tanh(*x);
    }
}

#[simd]
fn fast_sigmoid_inplace_kernel<S: Simd>(simd: S, buf: &mut [f32]) {
    let mut chunks = buf.chunks_exact_mut(S::f32s::LEN);
    for b in chunks.by_ref() {
        let x = S::f32s::from_slice(simd, b);
        fast_sigmoid_vec(simd, x).store_slice(b);
    }
    for x in chunks.into_remainder() {
        *x = fast_sigmoid(*x);
    }
}

#[simd]
fn relu_inplace_kernel<S: Simd>(simd: S, buf: &mut [f32]) {
    let zero = S::f32s::splat(simd, 0.0);
    let mut chunks = buf.chunks_exact_mut(S::f32s::LEN);
    for b in chunks.by_ref() {
        let x = S::f32s::from_slice(simd, b);
        x.max(zero).store_slice(b);
    }
    for x in chunks.into_remainder() {
        if *x < 0.0 {
            *x = 0.0;
        }
    }
}

#[simd]
fn hardtanh_inplace_kernel<S: Simd>(simd: S, buf: &mut [f32]) {
    let neg = S::f32s::splat(simd, -1.0);
    let pos = S::f32s::splat(simd, 1.0);
    let mut chunks = buf.chunks_exact_mut(S::f32s::LEN);
    for b in chunks.by_ref() {
        let x = S::f32s::from_slice(simd, b);
        x.max(neg).min(pos).store_slice(b);
    }
    for x in chunks.into_remainder() {
        *x = x.clamp(-1.0, 1.0);
    }
}

#[simd]
fn leaky_relu_inplace_kernel<S: Simd>(simd: S, buf: &mut [f32], negative_slope: f32) {
    let slope = S::f32s::splat(simd, negative_slope);
    let mut chunks = buf.chunks_exact_mut(S::f32s::LEN);
    for b in chunks.by_ref() {
        let x = S::f32s::from_slice(simd, b);
        let scaled = x * slope;
        x.simd_gt(0.0).select(x, scaled).store_slice(b);
    }
    for x in chunks.into_remainder() {
        if *x < 0.0 {
            *x *= negative_slope;
        }
    }
}

#[simd]
fn sigmoid_mul_add_inplace_kernel<S: Simd>(simd: S, dst: &mut [f32], src: &[f32], mul_src: &[f32]) {
    let n = dst.len().min(src.len()).min(mul_src.len());
    if n == 0 {
        return;
    }
    let dst = &mut dst[..n];
    let src = &src[..n];
    let mul_src = &mul_src[..n];
    let mut dchunks = dst.chunks_exact_mut(S::f32s::LEN);
    let mut schunks = src.chunks_exact(S::f32s::LEN);
    let mut mchunks = mul_src.chunks_exact(S::f32s::LEN);
    for ((d, s), m) in dchunks.by_ref().zip(schunks.by_ref()).zip(mchunks.by_ref()) {
        let dv = S::f32s::from_slice(simd, d);
        let sv = S::f32s::from_slice(simd, s);
        let mv = S::f32s::from_slice(simd, m);
        (dv + fast_sigmoid_vec(simd, sv) * mv).store_slice(d);
    }
    for ((d, s), m) in dchunks
        .into_remainder()
        .iter_mut()
        .zip(schunks.remainder())
        .zip(mchunks.remainder())
    {
        *d += fast_sigmoid(*s) * *m;
    }
}

#[simd]
fn soft_clip_inplace_kernel<S: Simd>(simd: S, buf: &mut [f32], knee: f32) {
    let range = 1.0 - knee;
    let len = S::f32s::LEN;
    let mut chunks = buf.chunks_exact_mut(len);
    for b in chunks.by_ref() {
        let x = S::f32s::from_slice(simd, b);
        let ax = x.abs();
        let below = ax.simd_le(knee);
        if below.all_true() {
            continue;
        }
        let over = (ax - knee) / range;
        let mut over_arr = [0.0f32; 16];
        over.store_slice(&mut over_arr[..len]);
        let mut clipped_arr = [0.0f32; 16];
        for (lane, o) in over_arr.iter().enumerate() {
            clipped_arr[lane] = knee + range * o.tanh();
        }
        let clipped = S::f32s::from_slice(simd, &clipped_arr[..len]);
        below.select(x, clipped.copysign(x)).store_slice(b);
    }
    for x in chunks.into_remainder() {
        *x = soft_clip_scalar(*x, knee);
    }
}

#[inline]
fn soft_clip_scalar(x: f32, knee: f32) -> f32 {
    let ax = x.abs();
    if ax <= knee {
        return x;
    }
    let over = (ax - knee) / (1.0 - knee);
    x.signum() * (knee + (1.0 - knee) * over.tanh())
}

#[simd]
fn softsign_inplace_kernel<S: Simd>(simd: S, buf: &mut [f32]) {
    let one = S::f32s::splat(simd, 1.0);
    let mut chunks = buf.chunks_exact_mut(S::f32s::LEN);
    for b in chunks.by_ref() {
        let x = S::f32s::from_slice(simd, b);
        (x / (one + x.abs())).store_slice(b);
    }
    for x in chunks.into_remainder() {
        *x /= 1.0 + x.abs();
    }
}

#[simd]
fn hardswish_inplace_kernel<S: Simd>(simd: S, buf: &mut [f32]) {
    let three = S::f32s::splat(simd, 3.0);
    let zero = S::f32s::splat(simd, 0.0);
    let six = S::f32s::splat(simd, 6.0);
    let inv6 = 1.0 / 6.0;
    let mut chunks = buf.chunks_exact_mut(S::f32s::LEN);
    for b in chunks.by_ref() {
        let x = S::f32s::from_slice(simd, b);
        let t = x + three;
        let clamped = t.max(zero).min(six);
        // Same association as the scalar reference: (x * clamped) * (1/6).
        ((x * clamped) * inv6).store_slice(b);
    }
    for x in chunks.into_remainder() {
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

    #[test]
    fn affine_inplace_matches_scalar() {
        let scale: Vec<f32> = (0..13).map(|i| 0.25 + i as f32 * 0.125).collect();
        let loc: Vec<f32> = (0..13).map(|i| 0.5 - i as f32 * 0.0625).collect();
        let base: Vec<f32> = (0..13).map(|i| (i % 5) as f32 * 0.4 - 0.8).collect();
        let mut buf = base.clone();
        affine_inplace(&mut buf, &scale, &loc);
        for i in 0..buf.len() {
            let expected = base[i] * scale[i] + loc[i];
            assert!((buf[i] - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn mul_per_sample_inplace_matches_scalar() {
        let src: Vec<f32> = (0..11).map(|i| (i % 4) as f32 * 0.5 - 0.75).collect();
        let mut buf = src.clone();
        mul_per_sample_inplace(&mut buf, &src);
        for i in 0..buf.len() {
            assert!((buf[i] - src[i] * src[i]).abs() < 1e-7);
        }
    }

    #[test]
    fn dot_product_matches_scalar() {
        let a: Vec<f32> = (0..29).map(|i| (i % 7) as f32 * 0.25 - 0.5).collect();
        let b: Vec<f32> = (0..29).map(|i| 1.0 - (i % 3) as f32 * 0.375).collect();
        let expected: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        assert!((dot_product(&a, &b) - expected).abs() < 1e-5);
    }

    #[test]
    fn ramp_scaled_matches_scalar() {
        let src: Vec<f32> = (0..17).map(|i| (i % 6) as f32 * 0.2 - 0.5).collect();
        let mut copied = vec![0.0f32; src.len()];
        copy_ramp_scaled_inplace(&mut copied, &src, 0.1, 1.3);
        let mut added = src.clone();
        add_ramp_scaled_inplace(&mut added, &src, 0.1, 1.3);
        let delta = (1.3 - 0.1) / (src.len() - 1) as f32;
        for i in 0..src.len() {
            let g = 0.1 + i as f32 * delta;
            assert!((copied[i] - src[i] * g).abs() < 1e-6);
            assert!((added[i] - (src[i] + src[i] * g)).abs() < 1e-6);
        }
    }

    #[test]
    fn soft_clip_inplace_matches_scalar() {
        let mut buf: Vec<f32> = (0..37).map(|i| (i % 13) as f32 * 0.31 - 1.9).collect();
        buf.extend([0.97, 0.98, 0.981, 1.0, 1.5, -0.98, -0.99, -2.0]);
        let knee = 0.98f32;
        let expected: Vec<f32> = buf
            .iter()
            .map(|&x| {
                let ax = x.abs();
                if ax <= knee {
                    x
                } else {
                    let over = (ax - knee) / (1.0 - knee);
                    x.signum() * (knee + (1.0 - knee) * over.tanh())
                }
            })
            .collect();
        soft_clip_inplace(&mut buf, knee);
        assert_eq!(buf, expected);
    }

    #[test]
    fn activations_match_scalar() {
        let base: Vec<f32> = (0..19).map(|i| (i % 9) as f32 * 0.5 - 2.0).collect();

        let mut buf = base.clone();
        fast_tanh_inplace(&mut buf);
        for i in 0..buf.len() {
            assert!((buf[i] - fast_tanh(base[i])).abs() < 1e-6);
        }

        let mut buf = base.clone();
        fast_sigmoid_inplace(&mut buf);
        for i in 0..buf.len() {
            assert!((buf[i] - fast_sigmoid(base[i])).abs() < 1e-6);
        }

        let mut buf = base.clone();
        relu_inplace(&mut buf);
        for i in 0..buf.len() {
            assert_eq!(buf[i], if base[i] < 0.0 { 0.0 } else { base[i] });
        }

        let mut buf = base.clone();
        hardtanh_inplace(&mut buf);
        for i in 0..buf.len() {
            assert_eq!(buf[i], base[i].clamp(-1.0, 1.0));
        }

        let mut buf = base.clone();
        leaky_relu_inplace(&mut buf, 0.1);
        for i in 0..buf.len() {
            let expected = if base[i] < 0.0 {
                base[i] * 0.1
            } else {
                base[i]
            };
            assert!((buf[i] - expected).abs() < 1e-7);
        }

        let mut buf = base.clone();
        softsign_inplace(&mut buf);
        for i in 0..buf.len() {
            assert!((buf[i] - base[i] / (1.0 + base[i].abs())).abs() < 1e-6);
        }

        let mut buf = base.clone();
        hardswish_inplace(&mut buf);
        for i in 0..buf.len() {
            let t = base[i] + 3.0;
            let expected = base[i] * t.clamp(0.0, 6.0) * (1.0 / 6.0);
            assert!((buf[i] - expected).abs() < 1e-6);
        }

        let mul: Vec<f32> = (0..19).map(|i| 0.5 + (i % 4) as f32 * 0.25).collect();
        let mut dst = base.clone();
        sigmoid_mul_add_inplace(&mut dst, &base, &mul);
        for i in 0..dst.len() {
            let expected = base[i] + fast_sigmoid(base[i]) * mul[i];
            assert!((dst[i] - expected).abs() < 1e-6);
        }
    }
}
