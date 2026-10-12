pub fn mul_gain_inplace(buf: &mut [f32], gain: f32) {
    crate::simd::mul_inplace(buf, gain);
}

pub fn clip_inplace(buf: &mut [f32], limit: f32) {
    crate::simd::clip_inplace(buf, limit);
}
