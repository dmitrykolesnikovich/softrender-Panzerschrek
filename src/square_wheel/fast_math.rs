// This module contains helper functions, based on various intrinsincs.

pub use fast_math_impl::*;

#[cfg(all(target_arch = "x86_64", target_feature = "sse4.1"))]
mod fast_math_impl
{
	use crate::common::color::*;
	use core::arch::x86_64::*;

	// Relative erorr <= 1.5 * 2^(-12)
	pub fn inv_sqrt_fast(x: f32) -> f32
	{
		unsafe { _mm_cvtss_f32(_mm_rsqrt_ss(_mm_set1_ps(x))) }
	}

	// Relative erorr <= 1.5 * 2^(-12)
	pub fn inv_fast(x: f32) -> f32
	{
		unsafe { _mm_cvtss_f32(_mm_rcp_ss(_mm_set1_ps(x))) }
	}

	pub fn color32_saturated_sum(a: Color32, b: Color32) -> Color32
	{
		unsafe {
			let a_in_register = _mm_cvtsi32_si128(a.get_raw() as i32);
			let b_in_register = _mm_cvtsi32_si128(b.get_raw() as i32);
			let sum = _mm_adds_epu8(a_in_register, b_in_register);
			Color32::from_raw(_mm_cvtsi128_si32(sum) as u32)
		}
	}

	pub fn color64_saturated_sum(a: Color64, b: Color64) -> Color64
	{
		unsafe {
			let a_in_register = _mm_cvtsi64_si128(a.get_raw() as i64);
			let b_in_register = _mm_cvtsi64_si128(b.get_raw() as i64);
			let sum = _mm_adds_epu16(a_in_register, b_in_register);
			Color64::from_raw(_mm_cvtsi128_si64(sum) as u64)
		}
	}

	pub fn color32_average(a: Color32, b: Color32) -> Color32
	{
		unsafe {
			let a_in_register = _mm_cvtsi32_si128(a.get_raw() as i32);
			let b_in_register = _mm_cvtsi32_si128(b.get_raw() as i32);
			let sum = _mm_avg_epu8(a_in_register, b_in_register);
			Color32::from_raw(_mm_cvtsi128_si32(sum) as u32)
		}
	}

	pub fn color64_average(a: Color64, b: Color64) -> Color64
	{
		unsafe {
			let a_in_register = _mm_cvtsi64_si128(a.get_raw() as i64);
			let b_in_register = _mm_cvtsi64_si128(b.get_raw() as i64);
			let sum = _mm_avg_epu16(a_in_register, b_in_register);
			Color64::from_raw(_mm_cvtsi128_si64(sum) as u64)
		}
	}

	// Pack 4 floats into 4 signed bytes.
	pub fn pack_f32x4_into_bytes(v: &[f32; 4], pack_scale: &[f32; 4]) -> i32
	{
		unsafe {
			let values_f = _mm_set_ps(v[3], v[2], v[1], v[0]);
			let scale = _mm_set_ps(pack_scale[3], pack_scale[2], pack_scale[1], pack_scale[0]);
			let values_scaled = _mm_mul_ps(values_f, scale);
			let values_32bit = _mm_cvtps_epi32(values_scaled);
			let zero = _mm_setzero_si128();
			let values_16bit = _mm_packs_epi32(values_32bit, zero);
			let values_8bit = _mm_packs_epi16(values_16bit, zero);
			let values_packed = _mm_cvtsi128_si32(values_8bit);
			values_packed
		}
	}

	// Unpak 4 signed bytes to floats.
	pub fn upack_bytes_into_f32x4(b: i32, unpack_scale: &[f32; 4]) -> [f32; 4]
	{
		unsafe {
			let values_8bit = _mm_cvtsi32_si128(b);
			let values_32bit = _mm_cvtepi8_epi32(values_8bit);
			let values_f4 = _mm_cvtepi32_ps(values_32bit);
			let scale = _mm_set_ps(unpack_scale[3], unpack_scale[2], unpack_scale[1], unpack_scale[0]);
			let values_scaled = _mm_mul_ps(values_f4, scale);
			[
				f32::from_bits(_mm_extract_ps(values_scaled, 0) as u32),
				f32::from_bits(_mm_extract_ps(values_scaled, 1) as u32),
				f32::from_bits(_mm_extract_ps(values_scaled, 2) as u32),
				f32::from_bits(_mm_extract_ps(values_scaled, 3) as u32),
			]
		}
	}

	#[repr(C, align(32))]
	#[derive(Copy, Clone)]
	pub struct ColorVec(__m128);

	impl ColorVec
	{
		pub fn zero() -> Self
		{
			unsafe { Self(_mm_setzero_ps()) }
		}

		pub fn from_color32(c: Color32) -> Self
		{
			unsafe {
				let color_32bit = c.get_raw() as i32;
				let values_8bit = _mm_cvtsi32_si128(color_32bit);
				let values_32bit = _mm_cvtepu8_epi32(values_8bit);
				let values_f4 = _mm_cvtepi32_ps(values_32bit);
				Self(values_f4)
			}
		}

		pub fn from_color64(c: Color64) -> Self
		{
			unsafe {
				let color_64bit = c.get_raw() as i64;
				let values_16bit = _mm_cvtsi64_si128(color_64bit);
				let values_32bit = _mm_cvtepu16_epi32(values_16bit);
				let values_f4 = _mm_cvtepi32_ps(values_32bit);
				Self(values_f4)
			}
		}

		pub fn into_color32(&self) -> Color32
		{
			// Here we 100% sure that components overflow is not possible (because of "min").
			// NaNs are not possible here too.
			unsafe {
				let max_val = 255.0;
				let values_clamped = _mm_min_ps(self.0, _mm_set_ps(max_val, max_val, max_val, max_val));
				let values_32bit = _mm_cvtps_epi32(values_clamped);
				let shuffle_mask = _mm_set_epi8(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 8, 4, 0);
				let values_8bit = _mm_shuffle_epi8(values_32bit, shuffle_mask);
				let color_32bit = _mm_cvtsi128_si32(values_8bit);
				Color32::from_raw(color_32bit as u32)
			}
		}

		pub fn into_color64(&self) -> Color64
		{
			// Here we 100% sure that components overflow is not possible (because of "min").
			// NaNs are not possible here too.
			unsafe {
				let max_val = 65535.0;
				let values_clamped = _mm_min_ps(self.0, _mm_set_ps(max_val, max_val, max_val, max_val));
				let values_32bit = _mm_cvtps_epi32(values_clamped);
				// TODO - check correctness of this mask.
				let shuffle_mask = _mm_set_epi8(0, 0, 0, 0, 0, 0, 0, 0, 13, 12, 9, 8, 5, 4, 1, 0);
				let values_16bit = _mm_shuffle_epi8(values_32bit, shuffle_mask);
				let color_64bit = _mm_cvtsi128_si64(values_16bit);
				Color64::from_raw(color_64bit as u64)
			}
		}

		pub fn from_color_f32x3(c: &[f32; 3]) -> Self
		{
			Self::from_color_f32x3_with_zero(c)
		}

		pub fn from_color_f32x3_with_zero(c: &[f32; 3]) -> Self
		{
			unsafe { Self(_mm_set_ps(0.0, c[0], c[1], c[2])) }
		}

		pub fn from_color_f32x3_with_one(c: &[f32; 3]) -> Self
		{
			unsafe { Self(_mm_set_ps(1.0, c[0], c[1], c[2])) }
		}

		pub fn into_color_f32x3(&self) -> [f32; 3]
		{
			// TODO - check this.
			unsafe {
				[
					f32::from_bits(_mm_extract_ps(self.0, 2) as u32),
					f32::from_bits(_mm_extract_ps(self.0, 1) as u32),
					f32::from_bits(_mm_extract_ps(self.0, 0) as u32),
				]
			}
		}

		pub fn add(&self, other: &Self) -> Self
		{
			unsafe { Self(_mm_add_ps(self.0, other.0)) }
		}

		pub fn mul(&self, other: &Self) -> Self
		{
			unsafe { Self(_mm_mul_ps(self.0, other.0)) }
		}

		pub fn scalar_mul(&self, scalar: f32) -> Self
		{
			unsafe { Self(_mm_mul_ps(self.0, _mm_broadcastss_ps(_mm_set1_ps(scalar)))) }
		}

		pub fn mul_scalar_add(&self, scalar: f32, b: &Self) -> Self
		{
			unsafe { Self(_mm_fmadd_ps(self.0, _mm_broadcastss_ps(_mm_set1_ps(scalar)), b.0)) }
		}

		pub fn mul_add(&self, b: &Self, c: &Self) -> Self
		{
			unsafe { Self(_mm_fmadd_ps(self.0, b.0, c.0)) }
		}

		pub fn div(&self, other: &Self) -> Self
		{
			unsafe { Self(_mm_div_ps(self.0, other.0)) }
		}

		pub fn insert<const INDEX: i32>(&mut self, scalar: f32)
		{
			unsafe {
				let scalar_vec = _mm_broadcastss_ps(_mm_set1_ps(scalar));
				match INDEX
				{
					0 =>
					{
						self.0 = _mm_blend_ps(self.0, scalar_vec, 1 << 0);
					},
					1 =>
					{
						self.0 = _mm_blend_ps(self.0, scalar_vec, 1 << 1);
					},
					2 =>
					{
						self.0 = _mm_blend_ps(self.0, scalar_vec, 1 << 2);
					},
					3 =>
					{
						self.0 = _mm_blend_ps(self.0, scalar_vec, 1 << 3);
					},
					_ =>
					{
						panic!("wrong index!")
					},
				}
			}
		}
	} // impl ColorVec

	impl From<ColorVecI> for ColorVec
	{
		fn from(v: ColorVecI) -> ColorVec
		{
			unsafe { ColorVec(_mm_cvtepi32_ps(v.0)) }
		}
	}

	#[repr(C, align(32))]
	#[derive(Copy, Clone)]
	pub struct ColorVecI(__m128i);

	impl ColorVecI
	{
		pub fn zero() -> Self
		{
			unsafe { Self(_mm_setzero_si128()) }
		}

		pub fn from_color_u32x3(c: &[u32; 3]) -> Self
		{
			unsafe { Self(_mm_set_epi32(0, c[0] as i32, c[1] as i32, c[2] as i32)) }
		}

		pub fn from_color_i32x3(c: &[i32; 3]) -> Self
		{
			Self::from_color_i32x3_with_zero(c)
		}

		pub fn from_color_i32x3_with_zero(c: &[i32; 3]) -> Self
		{
			unsafe { Self(_mm_set_epi32(0, c[0], c[1], c[2])) }
		}

		pub fn from_color_i32x3_with_one(c: &[i32; 3]) -> Self
		{
			unsafe { Self(_mm_set_epi32(1, c[0], c[1], c[2])) }
		}

		pub fn from_color_f32x3(c: &[f32; 3]) -> Self
		{
			unsafe {
				let f32_vec = _mm_set_ps(0.0, c[0], c[1], c[2]);
				Self(_mm_cvtps_epi32(f32_vec))
			}
		}

		pub fn from_color32(c: Color32) -> Self
		{
			unsafe {
				let color_32bit = c.get_raw() as i32;
				let values_16bit = _mm_cvtsi32_si128(color_32bit);
				let values_32bit = _mm_cvtepu8_epi32(values_16bit);
				Self(values_32bit)
			}
		}

		pub fn from_color64(c: Color64) -> Self
		{
			unsafe {
				let color_64bit = c.get_raw() as i64;
				let values_16bit = _mm_cvtsi64_si128(color_64bit);
				let values_32bit = _mm_cvtepu16_epi32(values_16bit);
				Self(values_32bit)
			}
		}

		pub fn into_color32(&self) -> Color32
		{
			unsafe {
				let zero = _mm_setzero_si128();
				let values_16bit = _mm_packus_epi32(self.0, zero);
				let values_8bit = _mm_packus_epi16(values_16bit, zero);
				let color_32bit = _mm_cvtsi128_si32(values_8bit);
				Color32::from_raw(color_32bit as u32)
			}
		}

		pub fn into_color64(&self) -> Color64
		{
			unsafe {
				let zero = _mm_setzero_si128();
				let values_16bit = _mm_packus_epi32(self.0, zero);
				let color_64bit = _mm_cvtsi128_si64(values_16bit);
				Color64::from_raw(color_64bit as u64)
			}
		}

		pub fn add(&self, other: &Self) -> Self
		{
			unsafe { Self(_mm_add_epi32(self.0, other.0)) }
		}

		pub fn mul(&self, other: &Self) -> Self
		{
			unsafe { Self(_mm_mullo_epi32(self.0, other.0)) }
		}

		pub fn mul_scalar(&self, scalar: i32) -> Self
		{
			unsafe { Self(_mm_mullo_epi32(self.0, _mm_set_epi32(scalar, scalar, scalar, scalar))) }
		}

		pub fn shift_left<const COUNT: i32>(&self) -> Self
		{
			unsafe { Self(_mm_slli_epi32(self.0, COUNT)) }
		}

		pub fn shift_right<const COUNT: i32>(&self) -> Self
		{
			unsafe { Self(_mm_srai_epi32(self.0, COUNT)) }
		}

		pub fn extract<const INDEX: i32>(&mut self) -> i32
		{
			unsafe { _mm_extract_epi32(self.0, INDEX) }
		}
	} // impl ColorVecI
}

#[cfg(not(all(target_arch = "x86_64", target_feature = "sse4.1")))]
mod fast_math_impl
{
	use super::*;
	use crate::common::color::*;

	pub fn inv_sqrt_fast(x: f32) -> f32
	{
		1.0 / x.sqrt()
	}

	pub fn inv_fast(x: f32) -> f32
	{
		1.0 / x
	}

	pub fn color32_saturated_sum(a: Color32, b: Color32) -> Color32
	{
		let mut r = 0;
		for i in 0 .. 4
		{
			let shift = i * 8;
			r |= (((a.get_raw() >> shift) & 0xFF) + ((b.get_raw() >> shift) & 0xFF)).min(0xFF) << shift;
		}
		Color32::from_raw(r)
	}

	pub fn color64_saturated_sum(a: Color64, b: Color64) -> Color64
	{
		let mut r = 0;
		for i in 0 .. 4
		{
			let shift = i * 16;
			r |= (((a.get_raw() >> shift) & 0xFFFF) + ((b.get_raw() >> shift) & 0xFFFF)).min(0xFFFF) << shift;
		}
		Color64::from_raw(r)
	}

	pub fn color32_average(a: Color32, b: Color32) -> Color32
	{
		Color32::get_average(a, b)
	}

	pub fn color64_average(a: Color64, b: Color64) -> Color64
	{
		Color64::get_average(a, b)
	}

	// Pack 4 floats into 4 signed bytes.
	pub fn pack_f32x4_into_bytes(v: &[f32; 4], pack_scale: &[f32; 4]) -> i32
	{
		let mut res = 0;
		for i in 0 .. 4
		{
			res |= (((v[i] * pack_scale[i]).max(-127.0).min(127.0) as i32) & 0xFF) << (i * 8);
		}
		res
	}

	// Unpak 4 signed bytes to floats.
	pub fn upack_bytes_into_f32x4(b: i32, unpack_scale: &[f32; 4]) -> [f32; 4]
	{
		let mut res = [0.0; 4];
		for i in 0 .. 4
		{
			res[i] = (((b << (24 - i * 8)) >> 24) as f32) * unpack_scale[i];
		}
		res
	}

	// TODO - maybe use here array of 3 floats?
	#[repr(C, align(32))]
	#[derive(Copy, Clone)]
	pub struct ColorVec([f32; 4]);

	impl ColorVec
	{
		pub fn zero() -> Self
		{
			Self([0.0, 0.0, 0.0, 0.0])
		}

		pub fn from_color32(c: Color32) -> Self
		{
			let mut res = [0.0; 4];
			for i in 0 .. 4
			{
				res[i] = ((c.get_raw() >> (i * 8)) & 0xFF) as f32;
			}
			Self(res)
		}

		pub fn from_color64(c: Color64) -> Self
		{
			let mut res = [0.0; 4];
			for i in 0 .. 4
			{
				res[i] = ((c.get_raw() >> (i * 16)) & 0xFFFF) as f32;
			}
			Self(res)
		}

		pub fn into_color32(&self) -> Color32
		{
			// Here we 100% sure that components overflow is not possible (because of "min").
			// NaNs are not possible here too.
			let mut res = 0;
			unsafe {
				for i in 0 .. 4
				{
					res |= self.0[i].min(255.0).to_int_unchecked::<u32>() << (i * 8);
				}
			}
			Color32::from_raw(res)
		}

		pub fn into_color64(&self) -> Color64
		{
			// Here we 100% sure that components overflow is not possible (because of "min").
			// NaNs are not possible here too.
			let mut res = 0;
			unsafe {
				for i in 0 .. 4
				{
					res |= self.0[i].min(65535.0).to_int_unchecked::<u64>() << (i * 16);
				}
			}
			Color64::from_raw(res)
		}

		pub fn from_color_f32x3(c: &[f32; 3]) -> Self
		{
			Self::from_color_f32x3_with_zero(c)
		}

		pub fn from_color_f32x3_with_zero(c: &[f32; 3]) -> Self
		{
			Self([c[2], c[1], c[0], 0.0])
		}

		pub fn from_color_f32x3_with_one(c: &[f32; 3]) -> Self
		{
			Self([c[2], c[1], c[0], 1.0])
		}

		pub fn into_color_f32x3(&self) -> [f32; 3]
		{
			[self.0[0], self.0[1], self.0[2]]
		}

		pub fn add(&self, other: &Self) -> Self
		{
			Self([
				self.0[0] + other.0[0],
				self.0[1] + other.0[1],
				self.0[2] + other.0[2],
				self.0[3] + other.0[3],
			])
		}

		pub fn mul(&self, other: &Self) -> Self
		{
			Self([
				self.0[0] * other.0[0],
				self.0[1] * other.0[1],
				self.0[2] * other.0[2],
				self.0[3] * other.0[3],
			])
		}

		pub fn scalar_mul(&self, scalar: f32) -> Self
		{
			Self([
				self.0[0] * scalar,
				self.0[1] * scalar,
				self.0[2] * scalar,
				self.0[3] * scalar,
			])
		}

		pub fn mul_scalar_add(&self, scalar: f32, b: &Self) -> Self
		{
			Self([
				f32_mul_add(self.0[0], scalar, b.0[0]),
				f32_mul_add(self.0[1], scalar, b.0[1]),
				f32_mul_add(self.0[2], scalar, b.0[2]),
				f32_mul_add(self.0[3], scalar, b.0[3]),
			])
		}

		pub fn mul_add(&self, b: &Self, c: &Self) -> Self
		{
			Self([
				f32_mul_add(self.0[0], b.0[0], c.0[0]),
				f32_mul_add(self.0[1], b.0[1], c.0[1]),
				f32_mul_add(self.0[2], b.0[2], c.0[2]),
				f32_mul_add(self.0[3], b.0[3], c.0[3]),
			])
		}

		pub fn div(&self, other: &Self) -> Self
		{
			Self([
				self.0[0] / other.0[0],
				self.0[1] / other.0[1],
				self.0[2] / other.0[2],
				self.0[3] / other.0[3],
			])
		}

		pub fn insert<const INDEX: i32>(&mut self, scalar: f32)
		{
			self.0[INDEX as usize] = scalar;
		}
	} // impl ColorVec

	impl From<ColorVecI> for ColorVec
	{
		fn from(v: ColorVecI) -> ColorVec
		{
			ColorVec([v.0[0] as f32, v.0[1] as f32, v.0[2] as f32, v.0[3] as f32])
		}
	}

	#[repr(C, align(32))]
	#[derive(Copy, Clone)]
	pub struct ColorVecI([i32; 4]);

	impl ColorVecI
	{
		pub fn zero() -> Self
		{
			Self([0; 4])
		}

		pub fn from_color_u32x3(c: &[u32; 3]) -> Self
		{
			Self([c[2] as i32, c[1] as i32, c[0] as i32, 0])
		}

		pub fn from_color_i32x3(c: &[i32; 3]) -> Self
		{
			Self::from_color_i32x3_with_zero(c)
		}

		pub fn from_color_i32x3_with_zero(c: &[i32; 3]) -> Self
		{
			Self([c[2], c[1], c[0], 0])
		}

		pub fn from_color_i32x3_with_one(c: &[i32; 3]) -> Self
		{
			Self([c[2], c[1], c[0], 1])
		}

		pub fn from_color_f32x3(c: &[f32; 3]) -> Self
		{
			Self([c[2] as i32, c[1] as i32, c[0] as i32, 0])
		}

		pub fn from_color32(c: Color32) -> Self
		{
			let mut res = [0; 4];
			for i in 0 .. 4
			{
				res[i] = ((c.get_raw() >> (i * 8)) & 0xFF) as i32;
			}
			Self(res)
		}

		pub fn from_color64(c: Color64) -> Self
		{
			let mut res = [0; 4];
			for i in 0 .. 4
			{
				res[i] = ((c.get_raw() >> (i * 16)) & 0xFFFF) as i32;
			}
			Self(res)
		}

		pub fn into_color32(&self) -> Color32
		{
			let mut res = 0;
			for i in 0 .. 4
			{
				res |= (self.0[i] as u32).min(255) << (i * 8);
			}
			Color32::from_raw(res)
		}

		pub fn into_color64(&self) -> Color64
		{
			let mut res = 0;
			for i in 0 .. 4
			{
				res |= (self.0[i] as u64).min(65535) << (i * 16);
			}
			Color64::from_raw(res)
		}

		pub fn add(&self, other: &Self) -> Self
		{
			let mut res = [0; 4];
			for i in 0 .. 4
			{
				res[i] = self.0[i] + other.0[i]
			}
			Self(res)
		}

		pub fn mul(&self, other: &Self) -> Self
		{
			let mut res = [0; 4];
			for i in 0 .. 4
			{
				res[i] = self.0[i] * other.0[i]
			}
			Self(res)
		}

		pub fn mul_scalar(&self, scalar: i32) -> Self
		{
			let mut res = [0; 4];
			for i in 0 .. 4
			{
				res[i] = self.0[i] * scalar
			}
			Self(res)
		}

		pub fn shift_left<const COUNT: i32>(&self) -> Self
		{
			let mut res = [0; 4];
			for i in 0 .. 4
			{
				res[i] = self.0[i] << COUNT
			}
			Self(res)
		}

		pub fn shift_right<const COUNT: i32>(&self) -> Self
		{
			let mut res = [0; 4];
			for i in 0 .. 4
			{
				res[i] = self.0[i] >> COUNT
			}
			Self(res)
		}

		pub fn extract<const INDEX: i32>(&mut self) -> i32
		{
			self.0[INDEX as usize]
		}
	} // impl ColorVecI
}

// On relatively modern CPUs FMA instruction is faster than sequential multiplication and addition. So, use native "mul_add" on such CPUs.
// But on old CPUs there is no such instruction, so, compiler uses ridiculously slow emulation. To avoid this just use combination of  multiplication and addition.

#[cfg(all(target_arch = "x86_64", target_feature = "fma"))]
pub fn f32_mul_add(x: f32, y: f32, z: f32) -> f32
{
	f32::mul_add(x, y, z)
}

#[cfg(not(all(target_arch = "x86_64", target_feature = "fma")))]
pub fn f32_mul_add(x: f32, y: f32, z: f32) -> f32
{
	x * y + z
}
