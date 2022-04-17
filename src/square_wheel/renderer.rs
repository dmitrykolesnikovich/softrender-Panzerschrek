use super::{clipping_polygon::*, rasterizer::*, renderer_config::*};
use common::{
	bsp_map_compact, camera_controller::CameraMatrices, clipping::*, color::*, fixed_math::*, image, math_types::*,
	system_window,
};

pub struct Renderer
{
	current_frame: FrameNumber,
	config: RendererConfig,
	map: bsp_map_compact::BSPMap,
	leafs_data: Vec<DrawLeafData>,
	portals_data: Vec<DrawPortalData>,
	polygons_data: Vec<DrawPolygonData>,
	surfaces_pixels: Vec<Color32>,
	leafs_search_waves: LeafsSearchWavesPair,
	textures: Vec<TextureWithMips>,
}

// Mutable data associated with map BSP Leaf.
#[derive(Default, Copy, Clone)]
struct DrawLeafData
{
	// Frame last time this leaf was visible.
	visible_frame: FrameNumber,
	// Bounds, combined from all paths through portals.
	current_frame_bounds: ClippingPolygon,
	num_search_visits: usize,
}

// Mutable data associated with map portal.
#[derive(Default, Copy, Clone)]
struct DrawPortalData
{
	// Frame last time this leaf was visible.
	visible_frame: FrameNumber,
	// None if behind camera.
	current_frame_projection: Option<ClippingPolygon>,
}

// Mutable data associated with map polygon.
#[derive(Default, Copy, Clone)]
struct DrawPolygonData
{
	// Frame last time this polygon was visible.
	visible_frame: FrameNumber,
	depth_equation: DepthEquation,
	tex_coord_equation: TexCoordEquation,
	surface_pixels_offset: usize,
	surface_size: [u32; 2],
}

// 32 bits are enough for frames enumeration.
// It is more than year at 60FPS.
#[derive(Default, Copy, Clone, PartialEq, Eq)]
struct FrameNumber(u32);

type LeafsSearchWaveElement = u32; // Leaf index
type LeafsSearchWave = Vec<LeafsSearchWaveElement>;
#[derive(Default)]
struct LeafsSearchWavesPair(LeafsSearchWave, LeafsSearchWave);

const MAX_MIP: usize = 3;
const NUM_MIPS: usize = MAX_MIP + 1;
type TextureWithMips = [image::Image; NUM_MIPS];

impl Renderer
{
	pub fn new(app_config: &serde_json::Value, map: bsp_map_compact::BSPMap) -> Self
	{
		let textures = load_textures(&map.textures);

		Renderer {
			current_frame: FrameNumber(0),
			config: RendererConfig::from_app_config(app_config),
			leafs_data: vec![DrawLeafData::default(); map.leafs.len()],
			portals_data: vec![DrawPortalData::default(); map.portals.len()],
			polygons_data: vec![DrawPolygonData::default(); map.polygons.len()],
			surfaces_pixels: Vec::new(),
			leafs_search_waves: LeafsSearchWavesPair::default(),
			map,
			textures,
		}
	}

	pub fn draw_frame(
		&mut self,
		pixels: &mut [Color32],
		surface_info: &system_window::SurfaceInfo,
		camera_matrices: &CameraMatrices,
	)
	{
		self.current_frame.0 += 1;

		if self.config.clear_background
		{
			draw_background(pixels);
		}

		let mut debug_stats = DebugStats::default();

		self.draw_map(pixels, surface_info, camera_matrices, &mut debug_stats);

		// TODO - remove such temporary fuinction.
		draw_crosshair(pixels, surface_info);

		if self.config.show_stats
		{
			let mut num_visible_leafs = 0;
			let mut max_search_visits = 0;
			for leaf_data in &self.leafs_data
			{
				if leaf_data.visible_frame == self.current_frame
				{
					num_visible_leafs += 1;
					max_search_visits = std::cmp::max(max_search_visits, leaf_data.num_search_visits);
				}
			}

			let mut num_visible_portals = 0;
			for portal_data in &self.portals_data
			{
				if portal_data.visible_frame == self.current_frame
				{
					num_visible_portals += 1;
				}
			}

			let mut num_visible_polygons = 0;
			for polygon_data in &self.polygons_data
			{
				if polygon_data.visible_frame == self.current_frame
				{
					num_visible_polygons += 1;
				}
			}

			common::text_printer::print(
				pixels,
				surface_info,
				&format!(
					"leafs: {}/{}\nportals: {}/{}\npolygons: {}\nsurfaces pixels: {}k\nnum reachable leaf search  \
					 calls: {}\nmax visits: {}\nmax reachable leaf search depth: {}\nmax reqachable leafs search wave \
					 size: {}",
					num_visible_leafs,
					self.leafs_data.len(),
					num_visible_portals,
					self.portals_data.len(),
					num_visible_polygons,
					(self.surfaces_pixels.len() + 1023) / 1024,
					debug_stats.num_reachable_leafs_search_calls,
					max_search_visits,
					debug_stats.reachable_leafs_search_calls_depth,
					debug_stats.reachable_leafs_search_max_wave_size,
				),
				0,
				0,
				Color32::from_rgb(255, 255, 255),
			);
		}
	}

	fn draw_map(
		&mut self,
		pixels: &mut [Color32],
		surface_info: &system_window::SurfaceInfo,
		camera_matrices: &CameraMatrices,
		debug_stats: &mut DebugStats,
	)
	{
		let mut rasterizer = Rasterizer::new(pixels, surface_info);
		let root_node = (self.map.nodes.len() - 1) as u32;
		let current_leaf = self.find_current_leaf(root_node, &camera_matrices.planes_matrix);

		// TODO - before preparing frame try to shift camera a little bit away from all planes of BSP nodes before current leaf.
		// This is needed to fix possible z_near clipping of current leaf portals.

		let frame_bounds = ClippingPolygon::from_box(0.0, 0.0, surface_info.width as f32, surface_info.height as f32);
		if self.config.recursive_visible_leafs_marking
		{
			mark_reachable_leafs_recursive(
				current_leaf,
				&self.map,
				self.current_frame,
				camera_matrices,
				0,
				&frame_bounds,
				&mut self.leafs_data,
				&mut self.portals_data,
				debug_stats,
			);
		}
		else
		{
			self.mark_reachable_leafs_iterative(current_leaf, camera_matrices, &frame_bounds, debug_stats);
		}

		self.prepare_polygons_surfaces(
			camera_matrices,
			&[
				rasterizer.get_width() as f32 * 0.5,
				rasterizer.get_height() as f32 * 0.5,
			],
		);

		// Draw BSP tree in back to front order, skip unreachable leafs.
		self.draw_tree_r(&mut rasterizer, camera_matrices, root_node);
	}

	fn find_current_leaf(&self, mut index: u32, planes_matrix: &Mat4f) -> u32
	{
		loop
		{
			if index >= bsp_map_compact::FIRST_LEAF_INDEX
			{
				return index - bsp_map_compact::FIRST_LEAF_INDEX;
			}

			let node = &self.map.nodes[index as usize];
			let plane_transformed = planes_matrix * node.plane.vec.extend(-node.plane.dist);
			index = if plane_transformed.w >= 0.0
			{
				node.children[0]
			}
			else
			{
				node.children[1]
			};
		}
	}

	fn mark_reachable_leafs_iterative(
		&mut self,
		start_leaf: u32,
		camera_matrices: &CameraMatrices,
		start_bounds: &ClippingPolygon,
		debug_stats: &mut DebugStats,
	)
	{
		debug_stats.reachable_leafs_search_max_wave_size = 0;

		let cur_wave = &mut self.leafs_search_waves.0;
		let next_wave = &mut self.leafs_search_waves.1;

		cur_wave.clear();
		next_wave.clear();

		cur_wave.push(start_leaf);
		self.leafs_data[start_leaf as usize].current_frame_bounds = *start_bounds;
		self.leafs_data[start_leaf as usize].visible_frame = self.current_frame;

		let mut depth = 0;
		while !cur_wave.is_empty()
		{
			for &leaf in cur_wave.iter()
			{
				debug_stats.num_reachable_leafs_search_calls += 1;

				let leaf_bounds = self.leafs_data[leaf as usize].current_frame_bounds;

				let leaf_value = self.map.leafs[leaf as usize];
				for &portal in &self.map.leafs_portals[(leaf_value.first_leaf_portal as usize) ..
					((leaf_value.first_leaf_portal + leaf_value.num_leaf_portals) as usize)]
				{
					let portal_value = &self.map.portals[portal as usize];

					// Do not look through portals that are facing from camera.
					let portal_plane_pos =
						(camera_matrices.planes_matrix * portal_value.plane.vec.extend(-portal_value.plane.dist)).w;

					let next_leaf;
					if portal_value.leafs[0] == leaf
					{
						if portal_plane_pos <= 0.0
						{
							continue;
						}
						next_leaf = portal_value.leafs[1];
					}
					else
					{
						if portal_plane_pos >= 0.0
						{
							continue;
						}
						next_leaf = portal_value.leafs[0];
					}

					// Same portal may be visited multiple times.
					// So, cache calculation of portal bounds.
					let portal_data = &mut self.portals_data[portal as usize];
					if portal_data.visible_frame != self.current_frame
					{
						portal_data.visible_frame = self.current_frame;
						portal_data.current_frame_projection =
							project_portal(portal_value, &self.map, &camera_matrices.view_matrix);
					}

					let mut bounds_intersection = if let Some(b) = portal_data.current_frame_projection
					{
						b
					}
					else
					{
						continue;
					};
					bounds_intersection.intersect(&leaf_bounds);
					if bounds_intersection.is_empty_or_invalid()
					{
						continue;
					}

					let next_leaf_data = &mut self.leafs_data[next_leaf as usize];
					if next_leaf_data.visible_frame != self.current_frame
					{
						next_leaf_data.visible_frame = self.current_frame;
						next_leaf_data.current_frame_bounds = bounds_intersection;
						next_leaf_data.num_search_visits = 1;
					}
					else
					{
						next_leaf_data.num_search_visits += 1;

						// If we visit this leaf not first time, check if bounds is inside current.
						// If so - we can skip it.
						if next_leaf_data.current_frame_bounds.contains(&bounds_intersection)
						{
							continue;
						}
						// Perform clipping of portals of this leaf using combined bounds to ensure that we visit all possible paths with such bounds.
						next_leaf_data.current_frame_bounds.extend(&bounds_intersection);
					}

					next_wave.push(next_leaf);
				} // For leaf portals.
			} // For wave elements.

			debug_stats.reachable_leafs_search_max_wave_size =
				std::cmp::max(debug_stats.reachable_leafs_search_max_wave_size, next_wave.len());

			cur_wave.clear();
			std::mem::swap(cur_wave, next_wave);

			depth += 1;
			if depth > 1024
			{
				// Prevent infinite loop in case of broken graph.
				break;
			}
		}
		debug_stats.reachable_leafs_search_calls_depth = depth;
	}

	fn prepare_polygons_surfaces(&mut self, camera_matrices: &CameraMatrices, viewport_half_size: &[f32; 2])
	{
		self.surfaces_pixels.clear();

		// TODO - try to speed-up iteration, do not scan all leafs.
		for i in 0 .. self.map.leafs.len()
		{
			let leaf_data = &self.leafs_data[i];
			if leaf_data.visible_frame == self.current_frame
			{
				let leaf = &self.map.leafs[i];
				// TODO - maybe just a little bit extend clipping polygon?
				let clip_planes = leaf_data.current_frame_bounds.get_clip_planes();
				for polygon_index in leaf.first_polygon .. (leaf.first_polygon + leaf.num_polygons)
				{
					self.prepare_polygon_surface(
						camera_matrices,
						&clip_planes,
						viewport_half_size,
						polygon_index as usize,
					);
				}
			}
		}
	}

	fn prepare_polygon_surface(
		&mut self,
		camera_matrices: &CameraMatrices,
		clip_planes: &ClippingPolygonPlanes,
		viewport_half_size: &[f32; 2],
		polygon_index: usize,
	)
	{
		let polygon = &self.map.polygons[polygon_index];
		let polygon_vertices = &self.map.vertices
			[(polygon.first_vertex as usize) .. ((polygon.first_vertex + polygon.num_vertices) as usize)];

		let plane_transformed = camera_matrices.planes_matrix * polygon.plane.vec.extend(-polygon.plane.dist);
		// Cull back faces.
		if plane_transformed.w <= 0.0
		{
			return;
		}

		// TODO - cache projected vertices and use them in rasterization.
		let mut vertices_2d = [Vec2f::zero(); MAX_VERTICES]; // TODO - use uninitialized memory
		let vertex_count = project_and_clip_polygon(
			&camera_matrices.view_matrix,
			clip_planes,
			polygon_vertices,
			&mut vertices_2d[..],
		);
		if vertex_count < 3
		{
			return;
		}

		let plane_transformed_w = -plane_transformed.w;
		let d_inv_z_dx = plane_transformed.x / plane_transformed_w;
		let d_inv_z_dy = plane_transformed.y / plane_transformed_w;
		let depth_equation = DepthEquation {
			d_inv_z_dx,
			d_inv_z_dy,
			k: plane_transformed.z / plane_transformed_w -
				d_inv_z_dx * viewport_half_size[0] -
				d_inv_z_dy * viewport_half_size[1],
		};

		let tex_coord_equation = &polygon.tex_coord_equation;

		// Calculate texture coordinates equations.
		let tc_basis_transformed = [
			camera_matrices.planes_matrix * tex_coord_equation[0].vec.extend(tex_coord_equation[0].dist),
			camera_matrices.planes_matrix * tex_coord_equation[1].vec.extend(tex_coord_equation[1].dist),
		];
		// Equation projeted to polygon plane.
		let tc_equation = TexCoordEquation {
			d_tc_dx: [
				tc_basis_transformed[0].x + tc_basis_transformed[0].w * depth_equation.d_inv_z_dx,
				tc_basis_transformed[1].x + tc_basis_transformed[1].w * depth_equation.d_inv_z_dx,
			],
			d_tc_dy: [
				tc_basis_transformed[0].y + tc_basis_transformed[0].w * depth_equation.d_inv_z_dy,
				tc_basis_transformed[1].y + tc_basis_transformed[1].w * depth_equation.d_inv_z_dy,
			],
			k: [
				tc_basis_transformed[0].z + tc_basis_transformed[0].w * depth_equation.k -
					tc_basis_transformed[0].x * viewport_half_size[0] -
					tc_basis_transformed[0].y * viewport_half_size[1],
				tc_basis_transformed[1].z + tc_basis_transformed[1].w * depth_equation.k -
					tc_basis_transformed[1].x * viewport_half_size[0] -
					tc_basis_transformed[1].y * viewport_half_size[1],
			],
		};

		let mip = calculate_mip(
			&vertices_2d[.. vertex_count],
			&depth_equation,
			&tc_equation,
			self.config.textures_mip_bias,
		);
		let tc_equation_scale = 1.0 / ((1 << mip) as f32);

		let tc_equation_scaled = TexCoordEquation {
			d_tc_dx: [
				tc_equation.d_tc_dx[0] * tc_equation_scale,
				tc_equation.d_tc_dx[1] * tc_equation_scale,
			],
			d_tc_dy: [
				tc_equation.d_tc_dy[0] * tc_equation_scale,
				tc_equation.d_tc_dy[1] * tc_equation_scale,
			],
			k: [
				tc_equation.k[0] * tc_equation_scale,
				tc_equation.k[1] * tc_equation_scale,
			],
		};

		// Calculate minimum/maximum texture coordinates.
		// Use clipped vertices for this.
		// With such approach we can allocate data only for visible part of surface, not whole polygon.
		let inf = 1.0e20;
		let mut tc_min = [inf, inf];
		let mut tc_max = [-inf, -inf];
		for p in &vertices_2d[.. vertex_count]
		{
			let z = 1.0 / (depth_equation.d_inv_z_dx * p.x + depth_equation.d_inv_z_dy * p.y + depth_equation.k);
			for i in 0 .. 2
			{
				let tc = z *
					(tc_equation_scaled.d_tc_dx[i] * p.x +
						tc_equation_scaled.d_tc_dy[i] * p.y +
						tc_equation_scaled.k[i]);
				if tc < tc_min[i]
				{
					tc_min[i] = tc;
				}
				if tc > tc_max[i]
				{
					tc_max[i] = tc;
				}
			}
		}

		// Reduce min/max texture coordinates slightly to avoid adding extra pixels
		// in case if min/max tex coord is exact integer, but slightly changed due to computational errors.
		let tc_reduce_eps = 1.0 / 32.0;
		for i in 0 .. 2
		{
			tc_min[i] += tc_reduce_eps;
			tc_max[i] -= tc_reduce_eps;
		}

		let tc_min_int = [tc_min[0].floor() as i32, tc_min[1].floor() as i32];
		let tc_max_int = [tc_max[0].ceil() as i32, tc_max[1].ceil() as i32];
		let surface_size = [
			(tc_max_int[0] - tc_min_int[0]).max(1),
			(tc_max_int[1] - tc_min_int[1]).max(1),
		];

		let surface_pixels_offset = self.surfaces_pixels.len();
		// TODO - avoid filling buffer with zeros.
		self.surfaces_pixels.resize(
			self.surfaces_pixels.len() + ((surface_size[0] * surface_size[1]) as usize),
			Color32::from_rgb(0, 0, 0),
		);

		let mip_texture = &self.textures[polygon.texture as usize][mip as usize];

		// TODO - perform surface data generation in separate step.
		for dst_y in 0 .. surface_size[1]
		{
			let dst_line_start = surface_pixels_offset + ((dst_y * surface_size[0]) as usize);
			let dst_line = &mut self.surfaces_pixels[dst_line_start .. dst_line_start + (surface_size[0] as usize)];

			let src_y = (tc_min_int[1] + dst_y).rem_euclid(mip_texture.size[1] as i32);
			let src_line_start = ((src_y as u32) * mip_texture.size[0]) as usize;
			let src_line = &mip_texture.pixels[src_line_start .. src_line_start + (mip_texture.size[0] as usize)];
			let mut src_x = tc_min_int[0].rem_euclid(mip_texture.size[0] as i32);
			for dst_x in 0 .. surface_size[0]
			{
				dst_line[dst_x as usize] = src_line[src_x as usize];
				src_x += 1;
				if src_x == (mip_texture.size[0] as i32)
				{
					src_x = 0;
				}
			}
		}

		let polygon_data = &mut self.polygons_data[polygon_index];
		polygon_data.visible_frame = self.current_frame;
		polygon_data.depth_equation = depth_equation;
		polygon_data.tex_coord_equation = tc_equation_scaled;
		polygon_data.surface_pixels_offset = surface_pixels_offset;
		polygon_data.surface_size = [surface_size[0] as u32, surface_size[1] as u32];

		// Correct texture coordinates equation to compensafe shift to surface rect.
		for i in 0 .. 2
		{
			let tc_min = tc_min_int[i] as f32;
			polygon_data.tex_coord_equation.d_tc_dx[i] -= tc_min * depth_equation.d_inv_z_dx;
			polygon_data.tex_coord_equation.d_tc_dy[i] -= tc_min * depth_equation.d_inv_z_dy;
			polygon_data.tex_coord_equation.k[i] -= tc_min * depth_equation.k;
		}
	}

	fn draw_tree_r(&self, rasterizer: &mut Rasterizer, camera_matrices: &CameraMatrices, current_index: u32)
	{
		if current_index >= bsp_map_compact::FIRST_LEAF_INDEX
		{
			let leaf = current_index - bsp_map_compact::FIRST_LEAF_INDEX;
			let leaf_data = &self.leafs_data[leaf as usize];
			if leaf_data.visible_frame == self.current_frame
			{
				self.draw_leaf(
					rasterizer,
					camera_matrices,
					&leaf_data.current_frame_bounds,
					&self.map.leafs[leaf as usize],
				);
			}
		}
		else
		{
			let node = &self.map.nodes[current_index as usize];
			let plane_transformed = camera_matrices.planes_matrix * node.plane.vec.extend(-node.plane.dist);
			let mut mask = if plane_transformed.w >= 0.0 { 1 } else { 0 };
			if self.config.invert_polygons_order
			{
				mask ^= 1;
			}
			for i in 0 .. 2
			{
				self.draw_tree_r(rasterizer, camera_matrices, node.children[(i ^ mask) as usize]);
			}
		}
	}

	fn draw_leaf(
		&self,
		rasterizer: &mut Rasterizer,
		camera_matrices: &CameraMatrices,
		bounds: &ClippingPolygon,
		leaf: &bsp_map_compact::BSPLeaf,
	)
	{
		// TODO - maybe just a little bit extend clipping polygon?
		let clip_planes = bounds.get_clip_planes();

		for polygon_index in leaf.first_polygon .. (leaf.first_polygon + leaf.num_polygons)
		{
			let polygon = &self.map.polygons[polygon_index as usize];
			let polygon_data = &self.polygons_data[polygon_index as usize];
			if polygon_data.visible_frame != self.current_frame
			{
				continue;
			}

			draw_polygon(
				rasterizer,
				camera_matrices,
				&clip_planes,
				&self.map.vertices
					[(polygon.first_vertex as usize) .. ((polygon.first_vertex + polygon.num_vertices) as usize)],
				&polygon_data.depth_equation,
				&polygon_data.tex_coord_equation,
				&polygon_data.surface_size,
				&self.surfaces_pixels[polygon_data.surface_pixels_offset ..
					polygon_data.surface_pixels_offset +
						((polygon_data.surface_size[0] * polygon_data.surface_size[1]) as usize)],
			);
		}
	}
}

fn draw_background(pixels: &mut [Color32])
{
	for pixel in pixels.iter_mut()
	{
		*pixel = Color32::from_rgb(32, 16, 8);
	}
}

fn draw_crosshair(pixels: &mut [Color32], surface_info: &system_window::SurfaceInfo)
{
	pixels[surface_info.width / 2 + surface_info.height / 2 * surface_info.pitch] = Color32::from_rgb(255, 255, 255);
}

fn load_textures(in_textures: &[bsp_map_compact::Texture]) -> Vec<TextureWithMips>
{
	let textures_dir = std::path::PathBuf::from("textures");
	let extension = ".tga";

	let mut result = Vec::new();

	for texture_name in in_textures
	{
		let null_pos = texture_name
			.iter()
			.position(|x| *x == 0_u8)
			.unwrap_or(texture_name.len());
		let range = &texture_name[0 .. null_pos];

		let texture_name_string = std::str::from_utf8(range).unwrap_or("").to_string();
		let texture_name_with_extension = texture_name_string + extension;

		let mut texture_path = textures_dir.clone();
		texture_path.push(texture_name_with_extension);

		let mip0 = if let Some(image) = image::load(&texture_path)
		{
			image
		}
		else
		{
			println!("Failed to load texture {:?}", texture_path);
			make_stub_texture()
		};

		result.push(build_mips(mip0));
	}

	result
}

fn make_stub_texture() -> image::Image
{
	let mut result = image::Image {
		size: [16, 16],
		pixels: vec![Color32::from_rgb(255, 0, 255); 16 * 16],
	};

	for y in 0 .. result.size[1]
	{
		for x in 0 .. result.size[0]
		{
			let color = if (((x >> 3) ^ (y >> 3)) & 1) != 0
			{
				Color32::from_rgb(255, 0, 255)
			}
			else
			{
				Color32::from_rgb(128, 128, 128)
			};
			result.pixels[(x + y * result.size[0]) as usize] = color;
		}
	}

	result
}

fn build_mips(mip0: image::Image) -> TextureWithMips
{
	// This function requires input texture with size multiple of ( 1 << MAX_MIP ).

	let mut result = TextureWithMips::default();

	result[0] = mip0;
	for i in 1 .. NUM_MIPS
	{
		let prev_mip = &mut result[i - 1];
		let mut mip = image::Image {
			size: [prev_mip.size[0] >> 1, prev_mip.size[1] >> 1],
			pixels: Vec::new(),
		};

		mip.pixels = vec![Color32::from_rgb(0, 0, 0); (mip.size[0] * mip.size[1]) as usize];
		for y in 0 .. mip.size[1] as usize
		{
			for x in 0 .. mip.size[0] as usize
			{
				mip.pixels[x + y * (mip.size[0] as usize)] = Color32::get_average_4([
					prev_mip.pixels[(x * 2) + (y * 2) * (prev_mip.size[0] as usize)],
					prev_mip.pixels[(x * 2) + (y * 2 + 1) * (prev_mip.size[0] as usize)],
					prev_mip.pixels[(x * 2 + 1) + (y * 2) * (prev_mip.size[0] as usize)],
					prev_mip.pixels[(x * 2 + 1) + (y * 2 + 1) * (prev_mip.size[0] as usize)],
				]);
			}
		}
		result[i] = mip;
	}

	result
}

// TODO - get rid of debug code.
#[derive(Default)]
struct DebugStats
{
	num_reachable_leafs_search_calls: usize,
	reachable_leafs_search_calls_depth: usize,
	reachable_leafs_search_max_wave_size: usize,
}

fn mark_reachable_leafs_recursive(
	leaf: u32,
	map: &bsp_map_compact::BSPMap,
	current_frame: FrameNumber,
	camera_matrices: &CameraMatrices,
	depth: u32,
	bounds: &ClippingPolygon,
	leafs_data: &mut [DrawLeafData],
	portals_data: &mut [DrawPortalData],
	debug_stats: &mut DebugStats,
)
{
	debug_stats.num_reachable_leafs_search_calls += 1;

	let max_depth = 1024; // Prevent stack overflow in case of broken graph.
	if depth > max_depth
	{
		return;
	}

	let leaf_data = &mut leafs_data[leaf as usize];

	if leaf_data.visible_frame != current_frame
	{
		leaf_data.visible_frame = current_frame;
		leaf_data.current_frame_bounds = *bounds;
		leaf_data.num_search_visits = 1;
	}
	else
	{
		leaf_data.num_search_visits += 1;

		// If we visit this leaf not first time, check if bounds is inside current.
		// If so - we can skip it.
		if leaf_data.current_frame_bounds.contains(bounds)
		{
			return;
		}
		// Perform clipping of portals of this leaf using combined bounds to ensure that we visit all possible paths with such bounds.
		leaf_data.current_frame_bounds.extend(bounds);
	}
	let bound_for_portals_clipping = leaf_data.current_frame_bounds;

	let leaf_value = map.leafs[leaf as usize];
	for portal in &map.leafs_portals[(leaf_value.first_leaf_portal as usize) ..
		((leaf_value.first_leaf_portal + leaf_value.num_leaf_portals) as usize)]
	{
		let portal_value = &map.portals[(*portal) as usize];

		// Do not look through portals that are facing from camera.
		let portal_plane_pos =
			(camera_matrices.planes_matrix * portal_value.plane.vec.extend(-portal_value.plane.dist)).w;

		let next_leaf;
		if portal_value.leafs[0] == leaf
		{
			if portal_plane_pos <= 0.0
			{
				continue;
			}
			next_leaf = portal_value.leafs[1];
		}
		else
		{
			if portal_plane_pos >= 0.0
			{
				continue;
			}
			next_leaf = portal_value.leafs[0];
		}

		// Same portal may be visited multiple times.
		// So, cache calculation of portal bounds.
		let portal_data = &mut portals_data[(*portal) as usize];
		if portal_data.visible_frame != current_frame
		{
			portal_data.visible_frame = current_frame;
			portal_data.current_frame_projection = project_portal(portal_value, map, &camera_matrices.view_matrix);
		}

		let mut bounds_intersection = if let Some(b) = portal_data.current_frame_projection
		{
			b
		}
		else
		{
			continue;
		};
		bounds_intersection.intersect(&bound_for_portals_clipping);
		if bounds_intersection.is_empty_or_invalid()
		{
			continue;
		}

		mark_reachable_leafs_recursive(
			next_leaf,
			map,
			current_frame,
			camera_matrices,
			depth + 1,
			&bounds_intersection,
			leafs_data,
			portals_data,
			debug_stats,
		);
	}
}

fn project_portal(
	portal: &bsp_map_compact::Portal,
	map: &bsp_map_compact::BSPMap,
	view_matrix: &Mat4f,
) -> Option<ClippingPolygon>
{
	const MAX_VERTICES: usize = 24;
	let mut vertex_count = std::cmp::min(portal.num_vertices as usize, MAX_VERTICES);

	// Perform initial matrix tranformation, obtain 3d vertices in camera-aligned space.
	let mut vertices_transformed = [Vec3f::zero(); MAX_VERTICES]; // TODO - use uninitialized memory
	for (in_vertex, out_vertex) in map.vertices
		[(portal.first_vertex as usize) .. (portal.first_vertex as usize) + vertex_count]
		.iter()
		.zip(vertices_transformed.iter_mut())
	{
		let vertex_transformed = view_matrix * in_vertex.extend(1.0);
		*out_vertex = Vec3f::new(vertex_transformed.x, vertex_transformed.y, vertex_transformed.w);
	}

	// Perform z_near clipping. Use very small z_near to avoid clipping portals.
	let mut vertices_transformed_z_clipped = [Vec3f::zero(); MAX_VERTICES]; // TODO - use uninitialized memory
	const Z_NEAR: f32 = 1.0 / 4096.0;
	vertex_count = clip_3d_polygon_by_z_plane(
		&vertices_transformed[.. vertex_count],
		Z_NEAR,
		&mut vertices_transformed_z_clipped,
	);
	if vertex_count < 3
	{
		return None;
	}

	let mut portal_polygon_bounds = ClippingPolygon::from_point(
		&(vertices_transformed_z_clipped[0].truncate() / vertices_transformed_z_clipped[0].z),
	);
	for vertex_transformed in &vertices_transformed_z_clipped[1 .. vertex_count]
	{
		portal_polygon_bounds.extend_with_point(&(vertex_transformed.truncate() / vertex_transformed.z));
	}

	Some(portal_polygon_bounds)
}

fn draw_polygon(
	rasterizer: &mut Rasterizer,
	camera_matrices: &CameraMatrices,
	clip_planes: &ClippingPolygonPlanes,
	vertices: &[Vec3f],
	depth_equation: &DepthEquation,
	tex_coord_equation: &TexCoordEquation,
	texture_size: &[u32; 2],
	texture_data: &[Color32],
)
{
	if vertices.len() < 3
	{
		return;
	}

	let mut vertices_2d = [Vec2f::zero(); MAX_VERTICES]; // TODO - use uninitialized memory
	let vertex_count = project_and_clip_polygon(
		&camera_matrices.view_matrix,
		clip_planes,
		vertices,
		&mut vertices_2d[..],
	);
	if vertex_count < 3
	{
		return;
	}

	// Perform f32 to Fixed16 conversion.
	let mut vertices_for_rasterizer = [PolygonPointProjected { x: 0, y: 0 }; MAX_VERTICES]; // TODO - use uninitialized memory
	for (index, vertex_2d) in vertices_2d.iter().enumerate().take(vertex_count)
	{
		vertices_for_rasterizer[index] = PolygonPointProjected {
			x: f32_to_fixed16(vertex_2d.x),
			y: f32_to_fixed16(vertex_2d.y),
		};
	}

	// Perform rasterization of fully clipped polygon.
	rasterizer.fill_polygon(
		&vertices_for_rasterizer[0 .. vertex_count],
		&depth_equation,
		&tex_coord_equation,
		&TextureInfo {
			size: [texture_size[0] as i32, texture_size[1] as i32],
		},
		texture_data,
	);
}

const MAX_VERTICES: usize = 24;

// Returns number of result vertices. < 3 if polygon is clipped.
fn project_and_clip_polygon(
	view_matrix: &Mat4f,
	clip_planes: &ClippingPolygonPlanes,
	vertices: &[Vec3f],
	out_vertices: &mut [Vec2f],
) -> usize
{
	let mut vertex_count = vertices.len();

	// Perform initial matrix tranformation, obtain 3d vertices in camera-aligned space.
	let mut vertices_transformed = [Vec3f::zero(); MAX_VERTICES]; // TODO - use uninitialized memory
															  // TODO - use "zip"?
	for (index, vertex) in vertices.iter().enumerate().take(MAX_VERTICES)
	{
		let vertex_transformed = view_matrix * vertex.extend(1.0);
		vertices_transformed[index] = Vec3f::new(vertex_transformed.x, vertex_transformed.y, vertex_transformed.w);
	}

	// Perform z_near clipping.
	let mut vertices_transformed_z_clipped = [Vec3f::zero(); MAX_VERTICES]; // TODO - use uninitialized memory
	const Z_NEAR: f32 = 1.0;
	vertex_count = clip_3d_polygon_by_z_plane(
		&vertices_transformed[.. vertex_count],
		Z_NEAR,
		&mut vertices_transformed_z_clipped,
	);
	if vertex_count < 3
	{
		return vertex_count;
	}

	// Make 2d vertices, then perform clipping in 2d space.
	// This is needed to avoid later overflows for Fixed16 vertex coords in rasterizer.
	// TODO - check for "out_vertices" bufer len.
	for (index, vertex_transformed) in vertices_transformed_z_clipped.iter().enumerate().take(vertex_count)
	{
		out_vertices[index] = vertex_transformed.truncate() / vertex_transformed.z;
	}

	let mut vertices_temp = [Vec2f::zero(); MAX_VERTICES]; // TODO - use uninitialized memory

	// Perform clipping in pairs - use pair of buffers.
	for i in 0 .. clip_planes.len() / 2
	{
		vertex_count = clip_2d_polygon(
			&out_vertices[.. vertex_count],
			&clip_planes[i * 2],
			&mut vertices_temp[..],
		);
		if vertex_count < 3
		{
			return vertex_count;
		}
		vertex_count = clip_2d_polygon(
			&vertices_temp[.. vertex_count],
			&clip_planes[i * 2 + 1],
			&mut out_vertices[..],
		);
		if vertex_count < 3
		{
			return vertex_count;
		}
	}

	vertex_count
}

fn calculate_mip(points: &[Vec2f], depth_equation: &DepthEquation, tc_equation: &TexCoordEquation, mip_bias: f32)
	-> u32
{
	// Calculate screen-space derivatives of texture coordinates for closest polygon point.
	// Calculate mip-level as logarithm of maximim texture coordinate component derivative.

	let mut mip_point = points[0];
	let mut mip_point_inv_z = 0.0;
	for &p in points
	{
		let inv_z = depth_equation.d_inv_z_dx * p.x + depth_equation.d_inv_z_dy * p.y + depth_equation.k;
		if inv_z > mip_point_inv_z
		{
			mip_point_inv_z = inv_z;
			mip_point = p;
		}
	}

	let z_2 = 1.0 / (mip_point_inv_z * mip_point_inv_z);
	let z_4 = z_2 * z_2;

	let mut d_tc_2: [f32; 2] = [0.0, 0.0];
	for i in 0 .. 2
	{
		let d_tc_dx = tc_equation.d_tc_dx[i] * (depth_equation.k + depth_equation.d_inv_z_dy * mip_point.y) -
			(tc_equation.k[i] + tc_equation.d_tc_dy[i] * mip_point.y) * depth_equation.d_inv_z_dx;
		let d_tc_dy = tc_equation.d_tc_dy[i] * (depth_equation.k + depth_equation.d_inv_z_dx * mip_point.x) -
			(tc_equation.k[i] + tc_equation.d_tc_dx[i] * mip_point.x) * depth_equation.d_inv_z_dy;

		d_tc_2[i] = (d_tc_dx * d_tc_dx + d_tc_dy * d_tc_dy) * z_4;
	}

	let max_d_tc_2 = d_tc_2[0].max(d_tc_2[1]);
	let mip_f = max_d_tc_2.log2() * 0.5 + mip_bias; // log(sqrt(x)) = log(x) * 0.5
	let mip = std::cmp::max(0, std::cmp::min(mip_f.ceil() as i32, MAX_MIP as i32));

	mip as u32
}
