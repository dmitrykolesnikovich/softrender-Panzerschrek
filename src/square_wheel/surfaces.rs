use super::light::*;
use common::{color::*, image, math_types::*, plane::*};

pub fn build_surface(
	surface_size: [u32; 2],
	surface_tc_min: [i32; 2],
	texture: &image::Image,
	plane: &Plane,
	tex_coord_equation: &[Plane; 2],
	lights: &[PointLight],
	out_surface_data: &mut [Color32],
)
{
	// Calculate inverse matrix for tex_coord aquation and plane equation in order to calculate world position for UV.
	// TODO - project tc equation to surface plane?
	let tex_coord_basis = Mat4f::from_cols(
		tex_coord_equation[0].vec.extend(tex_coord_equation[0].dist),
		tex_coord_equation[1].vec.extend(tex_coord_equation[1].dist),
		plane.vec.extend(-plane.dist),
		Vec4f::new(0.0, 0.0, 0.0, 1.0),
	);
	let tex_coord_basis_inverted = tex_coord_basis.transpose().invert().unwrap(); // TODO - avoid "unwrap"?

	let u_vec = tex_coord_basis_inverted.x.truncate();
	let v_vec = tex_coord_basis_inverted.y.truncate();
	let start_pos = tex_coord_basis_inverted.w.truncate() +
		u_vec * ((surface_tc_min[0]) as f32 + 0.5) +
		v_vec * ((surface_tc_min[1]) as f32 + 0.5);

	let plane_normal_normalized = plane.vec * inv_sqrt_fast(plane.vec.magnitude2());

	let constant_light = [1.5, 1.4, 1.3];

	for dst_v in 0 .. surface_size[1]
	{
		let dst_line_start = (dst_v * surface_size[0]) as usize;
		let dst_line = &mut out_surface_data[dst_line_start .. dst_line_start + (surface_size[0] as usize)];

		let src_v = (surface_tc_min[1] + (dst_v as i32)).rem_euclid(texture.size[1] as i32);
		let src_line_start = ((src_v as u32) * texture.size[0]) as usize;
		let src_line = &texture.pixels[src_line_start .. src_line_start + (texture.size[0] as usize)];
		let mut src_u = surface_tc_min[0].rem_euclid(texture.size[0] as i32);
		let mut dst_u = 0;
		let start_pos_v = start_pos + (dst_v as f32) * v_vec;
		for dst_texel in dst_line.iter_mut()
		{
			let pos = start_pos_v + (dst_u as f32) * u_vec;

			let mut total_light = constant_light;

			for light in lights
			{
				let vec_to_light = light.pos - pos;
				let vec_to_light_len2 = vec_to_light.magnitude2();
				let angle_cos = plane_normal_normalized.dot(vec_to_light) * inv_sqrt_fast(vec_to_light_len2);

				let light_scale = angle_cos.max(0.0) / vec_to_light_len2;

				total_light[0] += light.color[0] * light_scale;
				total_light[1] += light.color[1] * light_scale;
				total_light[2] += light.color[2] * light_scale;
			}

			let texel_value = src_line[src_u as usize];

			let components = texel_value.unpack_to_rgb_f32();
			let components_modulated = [
				(components[0] * total_light[0]).min(Color32::MAX_RGB_F32_COMPONENTS[0]),
				(components[1] * total_light[1]).min(Color32::MAX_RGB_F32_COMPONENTS[1]),
				(components[2] * total_light[2]).min(Color32::MAX_RGB_F32_COMPONENTS[2]),
			];

			// Here we 100% sure that components overflow is not possible (because of "min").
			// NaNs are not possible here too.
			let color_packed = unsafe { Color32::from_rgb_f32_unchecked(&components_modulated) };

			*dst_texel = color_packed;
			src_u += 1;
			if src_u == (texture.size[0] as i32)
			{
				src_u = 0;
			}

			dst_u += 1;
		}
	}
}

// Relative erorr <= 1.5 * 2^(-12)
#[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
fn inv_sqrt_fast(x: f32) -> f32
{
	unsafe { core::arch::x86_64::_mm_cvtss_f32(core::arch::x86_64::_mm_rsqrt_ss(core::arch::x86_64::_mm_set1_ps(x))) }
}

#[cfg(all(target_arch = "x86", target_feature = "sse"))]
fn inv_sqrt_fast(x: f32) -> f32
{
	unsafe { core::arch::x86::_mm_cvtss_f32(core::arch::x86::_mm_rsqrt_ss(core::arch::x86::_mm_set1_ps(x))) }
}

#[cfg(not(all(any(target_arch = "x86", target_arch = "x86_64"), target_feature = "sse")))]
fn inv_sqrt_fast(x: f32) -> f32
{
	1.0 / sqrt(x)
}
