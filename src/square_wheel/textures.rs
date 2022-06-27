use super::fast_math::*;
use common::{color::*, image, material, math_types::*};

// MAX_MIP must be not greater, than LIGHTMAP_SCALE_LOG2
pub const MAX_MIP: usize = 3;
pub const NUM_MIPS: usize = MAX_MIP + 1;
pub type TextureWithMips = [Texture; NUM_MIPS];

#[derive(Default, Clone)]
pub struct Texture
{
	pub size: [u32; 2],
	pub pixels: Vec<TextureElement>,
	pub has_normal_map: bool, // If false, normals data is trivial.
	pub has_non_one_roughness: bool,
	pub is_metal: bool,
}

#[derive(Copy, Clone)]
#[repr(C, align(8))] // TODO - tune usage of alignment.
pub struct TextureElement
{
	pub diffuse: Color32,
	pub packed_normal_roughness: PackedNormalRoughness,
}

#[derive(Copy, Clone)]
pub struct PackedNormalRoughness(i32);

impl PackedNormalRoughness
{
	pub fn pack(normal_normalized: &Vec3f, roughness: f32) -> Self
	{
		Self(pack_f32x4_into_bytes(
			&[normal_normalized.x, normal_normalized.y, normal_normalized.z, roughness],
			&[NORMAL_SCALE, NORMAL_SCALE, NORMAL_SCALE, ROUGHNESS_SCALE],
		))
	}

	pub fn unpack(&self) -> (Vec3f, f32)
	{
		let values_unpacked = upack_bytes_into_f32x4(
			self.0,
			&[
				1.0 / NORMAL_SCALE,
				1.0 / NORMAL_SCALE,
				1.0 / NORMAL_SCALE,
				1.0 / ROUGHNESS_SCALE,
			],
		);
		(
			Vec3f::new(values_unpacked[0], values_unpacked[1], values_unpacked[2]),
			values_unpacked[3],
		)
	}

	pub fn unpack_roughness(&self) -> f32
	{
		(self.0 >> 24) as f32 * (1.0 / ROUGHNESS_SCALE)
	}
}

const NORMAL_SCALE: f32 = 127.0;
const ROUGHNESS_SCALE: f32 = 127.0;

impl Default for TextureElement
{
	fn default() -> Self
	{
		Self {
			diffuse: Color32::black(),
			packed_normal_roughness: PackedNormalRoughness::pack(&Vec3f::unit_z(), 1.0),
		}
	}
}

pub fn load_textures(materials: &[material::Material], textures_path: &std::path::Path) -> Vec<TextureWithMips>
{
	let mut result = Vec::new();

	for material in materials
	{
		let diffuse = if let Some(image) = load_image(
			&material.diffuse.clone().unwrap_or_else(|| String::new()),
			textures_path,
		)
		{
			image
		}
		else
		{
			make_stub_image()
		};

		let normals = if let Some(normal_map_texture) = &material.normal_map
		{
			load_image(&normal_map_texture.clone(), textures_path)
		}
		else
		{
			None
		};

		let roughness_map = if let Some(roughness_map_texture) = &material.roughness_map
		{
			load_image(&roughness_map_texture.clone(), textures_path)
		}
		else
		{
			None
		};

		let mip0 = make_texture(diffuse, normals, material.roughness, roughness_map, material.is_metal);

		result.push(build_mips(mip0));
	}

	result
}

fn load_image(file_name: &str, textures_path: &std::path::Path) -> Option<image::Image>
{
	let mut path = std::path::PathBuf::from(textures_path);
	path.push(file_name);
	image::load(&path)
}

fn make_stub_image() -> image::Image
{
	let size = 32;
	let mut result = image::Image {
		size: [size, size],
		pixels: vec![Color32::from_rgb(0, 0, 0); (size * size) as usize],
	};

	for y in 0 .. result.size[1]
	{
		for x in 0 .. result.size[0]
		{
			let color = if (((x >> 3) ^ (y >> 3)) & 1) != 0
			{
				Color32::from_rgb(224, 224, 224)
			}
			else
			{
				Color32::from_rgb(160, 160, 160)
			};
			result.pixels[(x + y * result.size[0]) as usize] = color;
		}
	}

	result
}

fn make_texture(
	diffuse: image::Image,
	mut normals: Option<image::Image>,
	roughness: f32,
	mut roughness_map: Option<image::Image>,
	is_metal: bool,
) -> Texture
{
	let roughness_clamped = roughness.max(0.0).min(1.0);

	let mut result = Texture {
		size: diffuse.size,
		pixels: vec![TextureElement::default(); (diffuse.size[0] * diffuse.size[1]) as usize],
		has_normal_map: normals.is_some(),
		has_non_one_roughness: roughness_clamped < 1.0 || roughness_map.is_some(),
		is_metal,
	};

	if let Some(n) = &mut normals
	{
		if n.size != diffuse.size
		{
			let n_resized = resize_image(&n, diffuse.size);
			*n = n_resized;
		}
	}
	if let Some(r) = &mut roughness_map
	{
		if r.size != diffuse.size
		{
			let r_resized = resize_image(&r, diffuse.size);
			*r = r_resized;
		}
	}

	for (index, (dst, src)) in result.pixels.iter_mut().zip(diffuse.pixels.iter()).enumerate()
	{
		dst.diffuse = *src;

		let normal = if let Some(n) = &normals
		{
			let rgb = n.pixels[index].get_rgb();
			let zero_level = 128;
			let normal = Vec3f::new(
				((rgb[0] as i32) - zero_level) as f32,
				((rgb[1] as i32) - zero_level) as f32,
				((rgb[2] as i32) - zero_level) as f32,
			);
			renormalize_normal(normal)
		}
		else
		{
			Vec3f::unit_z()
		};

		let roughness = if let Some(r) = &roughness_map
		{
			r.pixels[index].get_rgb()[0] as f32 / 255.0
		}
		else
		{
			roughness_clamped
		}
		.max(MIN_VALID_ROUGHNESS);

		dst.packed_normal_roughness = PackedNormalRoughness::pack(&normal, roughness);
	}

	result
}

// Resize with simple nearset filter.
fn resize_image(image: &image::Image, target_size: [u32; 2]) -> image::Image
{
	let mut result = image::Image {
		size: target_size,
		pixels: vec![Color32::black(); (target_size[0] * target_size[1]) as usize],
	};

	for y in 0 .. result.size[1]
	{
		let src_y = y * image.size[1] / result.size[1];
		for x in 0 .. result.size[0]
		{
			let src_x = x * image.size[0] / result.size[0];
			result.pixels[(x + y * result.size[0]) as usize] = image.pixels[(src_x + src_y * image.size[0]) as usize];
		}
	}
	result
}

fn build_mips(mip0: Texture) -> TextureWithMips
{
	// This function requires input texture with size multiple of ( 1 << MAX_MIP ).

	let mut result = TextureWithMips::default();

	result[0] = mip0;
	for i in 1 .. NUM_MIPS
	{
		let prev_mip = &mut result[i - 1];
		let mut mip = Texture {
			size: [prev_mip.size[0] >> 1, prev_mip.size[1] >> 1],
			pixels: Vec::new(),
			has_normal_map: prev_mip.has_normal_map,
			has_non_one_roughness: prev_mip.has_non_one_roughness,
			is_metal: prev_mip.is_metal,
		};

		mip.pixels = vec![TextureElement::default(); (mip.size[0] * mip.size[1]) as usize];

		let prev_mip_width = prev_mip.size[0] as usize;
		let mip_width = mip.size[0] as usize;
		for y in 0 .. mip.size[1] as usize
		{
			let src_offset0 = (y * 2) * prev_mip_width;
			let src_offset1 = (y * 2 + 1) * prev_mip_width;
			for (dst, x) in mip.pixels[y * mip_width .. (y + 1) * mip_width]
				.iter_mut()
				.zip(0 .. mip_width)
			{
				let src_x = x * 2;
				let p00 = prev_mip.pixels[src_x + src_offset0];
				let p01 = prev_mip.pixels[src_x + src_offset1];
				let p10 = prev_mip.pixels[src_x + 1 + src_offset0];
				let p11 = prev_mip.pixels[src_x + 1 + src_offset1];

				dst.diffuse = Color32::get_average_4([p00.diffuse, p01.diffuse, p10.diffuse, p11.diffuse]);

				let (p00_normal, p00_roughness) = p00.packed_normal_roughness.unpack();
				let (p01_normal, p01_roughness) = p01.packed_normal_roughness.unpack();
				let (p10_normal, p10_roughness) = p10.packed_normal_roughness.unpack();
				let (p11_normal, p11_roughness) = p11.packed_normal_roughness.unpack();

				let normals_sum = p00_normal + p01_normal + p10_normal + p11_normal;
				let normals_lens_sum =
					(p00_normal.magnitude() + p01_normal.magnitude() + p10_normal.magnitude() + p11_normal.magnitude())
						.max(0.000001);
				let normals_sum_len = normals_sum.magnitude().max(0.000001);

				let dst_normal = normals_sum / normals_sum_len;

				// Increase roughness proportional to deviation of normal.
				let half_normal_deviation_cos = normals_sum_len / normals_lens_sum;
				const MIN_HALF_NORMAL_DEVIATION_COS: f32 = 0.5;
				let normal_deviation = (1.0 - half_normal_deviation_cos) / (1.0 - MIN_HALF_NORMAL_DEVIATION_COS);

				let average_roughness = (p00_roughness + p01_roughness + p10_roughness + p11_roughness) * 0.25;
				let dst_roughness = (average_roughness + normal_deviation).max(MIN_VALID_ROUGHNESS).min(1.0);

				dst.packed_normal_roughness = PackedNormalRoughness::pack(&dst_normal, dst_roughness);
			}
		}
		result[i] = mip;
	}

	result
}

// Do not allow absolte zero roughness. Limit this value to integer 1 in compressed format.
const MIN_VALID_ROUGHNESS: f32 = 1.0 / (ROUGHNESS_SCALE - 1.0);

fn renormalize_normal(normal: Vec3f) -> Vec3f
{
	let len = normal.magnitude();
	if len <= 0.000001
	{
		Vec3f::unit_z()
	}
	else
	{
		normal / len
	}
}
