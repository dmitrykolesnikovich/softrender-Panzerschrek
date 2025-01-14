use super::{bsp_builder, bsp_map_compact::*, map_polygonizer, material::*, math_types::*};
use std::collections::HashMap;

pub fn convert_bsp_map_to_compact_format(
	bsp_tree: &bsp_builder::BSPTree,
	entities: &[map_polygonizer::Entity],
	// Excluding world entity.
	entites_bsp_trees: &[bsp_builder::SubmodelBSPNode],
	materials: &MaterialsMap,
) -> BSPMap
{
	let mut out_map = BSPMap::default();

	let mut portal_ptr_to_index_map = PortalPtrToIndexMap::new();
	convert_portals_to_compact_format(&bsp_tree.portals, &mut out_map, &mut portal_ptr_to_index_map);

	let mut leaf_ptr_to_index_map = LeafPtrToIndexMap::new();
	let mut texture_name_to_index_map = TextureNameToIndexMap::new();
	convert_node_child_to_compact_format(
		&bsp_tree.root,
		&portal_ptr_to_index_map,
		materials,
		&mut out_map,
		&mut leaf_ptr_to_index_map,
		&mut texture_name_to_index_map,
	);

	fill_portals_leafs(&bsp_tree.portals, &leaf_ptr_to_index_map, &mut out_map);

	convert_entities_to_compact_format(
		entities,
		entites_bsp_trees,
		materials,
		&mut out_map,
		&mut texture_name_to_index_map,
	);

	fill_textures(&texture_name_to_index_map, &mut out_map);

	out_map
}

type PortalPtrToIndexMap = HashMap<*const bsp_builder::LeafsPortal, u32>;

fn convert_portals_to_compact_format(
	portals: &[bsp_builder::LeafsPortalPtr],
	out_map: &mut BSPMap,
	portal_ptr_to_index_map: &mut PortalPtrToIndexMap,
)
{
	for portal_ptr in portals
	{
		let portal_index = out_map.portals.len() as u32;
		let portal = portal_ptr.borrow();
		let portal_raw_ptr = (&*portal) as *const bsp_builder::LeafsPortal;
		portal_ptr_to_index_map.insert(portal_raw_ptr, portal_index);

		let portal_converted = convert_portal_to_compact_format(portal_ptr, out_map);
		out_map.portals.push(portal_converted);
	}
}

fn convert_portal_to_compact_format(portal_ptr: &bsp_builder::LeafsPortalPtr, out_map: &mut BSPMap) -> Portal
{
	let portal = portal_ptr.borrow();
	let first_vertex = out_map.vertices.len() as u32;
	out_map.vertices.extend_from_slice(&portal.vertices);
	Portal {
		first_vertex,
		num_vertices: portal.vertices.len() as u32,
		// Fill leafs later.
		leafs: [0, 0],
		plane: portal.plane,
	}
}

type LeafPtrToIndexMap = HashMap<*const bsp_builder::BSPLeaf, u32>;
type TextureNameToIndexMap = HashMap<String, u32>;

// Returns index of child.
fn convert_node_child_to_compact_format(
	node_child: &bsp_builder::BSPNodeChild,
	portal_ptr_to_index_map: &PortalPtrToIndexMap,
	materials: &MaterialsMap,
	out_map: &mut BSPMap,
	leaf_ptr_to_index_map: &mut LeafPtrToIndexMap,
	texture_name_to_index_map: &mut TextureNameToIndexMap,
) -> u32
{
	match node_child
	{
		bsp_builder::BSPNodeChild::NodeChild(node_ptr) =>
		{
			let node_converted = convert_node_to_compact_format(
				node_ptr,
				portal_ptr_to_index_map,
				materials,
				out_map,
				leaf_ptr_to_index_map,
				texture_name_to_index_map,
			);
			let node_index = out_map.nodes.len();
			out_map.nodes.push(node_converted);
			node_index as u32
		},
		bsp_builder::BSPNodeChild::LeafChild(leaf_ptr) =>
		{
			let leaf_index = out_map.leafs.len();
			let leaf_converted = convert_leaf_to_compact_format(
				leaf_ptr,
				portal_ptr_to_index_map,
				materials,
				out_map,
				texture_name_to_index_map,
			);
			out_map.leafs.push(leaf_converted);

			let leaf = leaf_ptr.borrow();
			let leaf_raw_ptr = (&*leaf) as *const bsp_builder::BSPLeaf;
			leaf_ptr_to_index_map.insert(leaf_raw_ptr, leaf_index as u32);

			(leaf_index as u32) + FIRST_LEAF_INDEX
		},
	}
}

fn convert_node_to_compact_format(
	node_ptr: &bsp_builder::BSPNodePtr,
	portal_ptr_to_index_map: &PortalPtrToIndexMap,
	materials: &MaterialsMap,
	out_map: &mut BSPMap,
	leaf_ptr_to_index_map: &mut LeafPtrToIndexMap,
	texture_name_to_index_map: &mut TextureNameToIndexMap,
) -> BSPNode
{
	let node = node_ptr.borrow();
	let child0 = convert_node_child_to_compact_format(
		&node.children[0],
		portal_ptr_to_index_map,
		materials,
		out_map,
		leaf_ptr_to_index_map,
		texture_name_to_index_map,
	);
	let child1 = convert_node_child_to_compact_format(
		&node.children[1],
		portal_ptr_to_index_map,
		materials,
		out_map,
		leaf_ptr_to_index_map,
		texture_name_to_index_map,
	);
	BSPNode {
		children: [child0, child1],
		plane: node.plane,
	}
}

fn convert_leaf_to_compact_format(
	leaf_ptr: &bsp_builder::BSPLeafPtr,
	portal_ptr_to_index_map: &PortalPtrToIndexMap,
	materials: &MaterialsMap,
	out_map: &mut BSPMap,
	texture_name_to_index_map: &mut TextureNameToIndexMap,
) -> BSPLeaf
{
	let leaf = leaf_ptr.borrow();

	let polygons_splitted = bsp_builder::split_long_polygons(&leaf.polygons, materials);

	let first_polygon = out_map.polygons.len() as u32;
	for polygon in &polygons_splitted
	{
		let polygon_converted = convert_polygon_to_compact_format(&polygon, out_map, texture_name_to_index_map);
		out_map.polygons.push(polygon_converted);
	}

	let first_leaf_portal = out_map.leafs_portals.len() as u32;
	for portal_weak_ptr in &leaf.portals
	{
		let portal_ptr = portal_weak_ptr.upgrade().unwrap();
		let portal = portal_ptr.borrow();
		let portal_raw_ptr = (&*portal) as *const bsp_builder::LeafsPortal;
		let portal_index = portal_ptr_to_index_map.get(&portal_raw_ptr).unwrap();
		out_map.leafs_portals.push(*portal_index);
	}

	BSPLeaf {
		first_polygon,
		num_polygons: polygons_splitted.len() as u32,
		first_leaf_portal,
		num_leaf_portals: leaf.portals.len() as u32,
	}
}

fn convert_polygon_to_compact_format(
	polygon: &bsp_builder::Polygon,
	out_map: &mut BSPMap,
	texture_name_to_index_map: &mut TextureNameToIndexMap,
) -> Polygon
{
	let first_vertex = out_map.vertices.len() as u32;
	out_map.vertices.extend_from_slice(&polygon.vertices);

	let inf = (1 << 29) as f32;
	let mut tc_min = [inf, inf];
	let mut tc_max = [-inf, -inf];
	for &vertex in &polygon.vertices
	{
		for i in 0 .. 2
		{
			let tc = polygon.texture_info.tex_coord_equation[i].vec.dot(vertex) +
				polygon.texture_info.tex_coord_equation[i].dist;
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

	for i in 0 .. 2
	{
		// Reduce min/max texture coordinates slightly to avoid adding extra pixels
		// in case if min/max tex coord is exact integer, but slightly changed due to computational errors.
		let tc_reduce_eps = 1.0 / 32.0;
		tc_min[i] += tc_reduce_eps;
		tc_max[i] -= tc_reduce_eps;
	}

	let tex_coord_min = [tc_min[0].floor() as i32, tc_min[1].floor() as i32];
	let tex_coord_max = [
		(tc_max[0].ceil() as i32).max(tex_coord_min[0] + 1),
		(tc_max[1].ceil() as i32).max(tex_coord_min[1] + 1),
	];

	Polygon {
		first_vertex,
		num_vertices: polygon.vertices.len() as u32,
		plane: polygon.plane,
		tex_coord_equation: polygon.texture_info.tex_coord_equation,
		tex_coord_min,
		tex_coord_max,
		lightmap_data_offset: 0, // Fill this later, during lightmaps build.
		texture: get_texture_index(&polygon.texture_info.texture, texture_name_to_index_map),
	}
}

fn get_texture_index(texture_name: &String, texture_name_to_index_map: &mut TextureNameToIndexMap) -> u32
{
	if let Some(index) = texture_name_to_index_map.get(texture_name)
	{
		return *index;
	}
	let index = texture_name_to_index_map.len() as u32;
	texture_name_to_index_map.insert(texture_name.clone(), index);
	index
}

fn fill_portals_leafs(
	portals: &[bsp_builder::LeafsPortalPtr],
	leaf_to_index_map: &LeafPtrToIndexMap,
	out_map: &mut BSPMap,
)
{
	if portals.len() != out_map.portals.len()
	{
		panic!("Portal count mismatch!");
	}

	for (portal_index, out_portal) in out_map.portals.iter_mut().enumerate()
	{
		let portal_ptr = &portals[portal_index];
		let portal = portal_ptr.borrow();

		out_portal.leafs[0] = get_leaf_index(&portal.leaf_front, leaf_to_index_map);
		out_portal.leafs[1] = get_leaf_index(&portal.leaf_back, leaf_to_index_map);
	}
}

fn get_leaf_index(leaf_ptr: &bsp_builder::BSPLeafPtr, leaf_to_index_map: &LeafPtrToIndexMap) -> u32
{
	let leaf = leaf_ptr.borrow();
	let leaf_raw_ptr = (&*leaf) as *const bsp_builder::BSPLeaf;
	*leaf_to_index_map.get(&leaf_raw_ptr).unwrap()
}

fn fill_textures(texture_name_to_index_map: &TextureNameToIndexMap, out_map: &mut BSPMap)
{
	out_map.textures = vec![[0; MAX_TEXTURE_NAME_LEN]; texture_name_to_index_map.len()];
	for (name, index) in texture_name_to_index_map
	{
		let name_bytes = name.as_bytes();

		let out_texture_bytes = &mut out_map.textures[(*index) as usize];

		// ".." operator will panic in case of name overflow.
		out_texture_bytes[0 .. name_bytes.len()].copy_from_slice(name_bytes);
	}
}

fn convert_entities_to_compact_format(
	entities: &[map_polygonizer::Entity],
	// Excluding world entity.
	entites_bsp_trees: &[bsp_builder::SubmodelBSPNode],
	materials: &MaterialsMap,
	out_map: &mut BSPMap,
	texture_name_to_index_map: &mut TextureNameToIndexMap,
)
{
	let mut strings_cache = StringsCache::new();
	for (entity_index, entity) in entities.iter().enumerate()
	{
		let mut submodel_index = !0;
		if entity_index != 0
		{
			// Create submodels for entities with non-empty BSP tree. Skip entity #0 - world entity.
			let entity_bsp_tree = &entites_bsp_trees[entity_index - 1];
			if !entity_bsp_tree.polygons.is_empty() ||
				entity_bsp_tree.children[0].is_some() ||
				entity_bsp_tree.children[1].is_some()
			{
				submodel_index = out_map.submodels.len() as u32;
				let submodel_converted = convert_submodel_bsp_tree_to_compact_format(
					entity_bsp_tree,
					materials,
					out_map,
					texture_name_to_index_map,
				);
				out_map.submodels.push(submodel_converted);
			}
		}

		let entity_converted = convert_entity_to_compact_format(entity, submodel_index, out_map, &mut strings_cache);
		out_map.entities.push(entity_converted);
	}
}

fn convert_submodel_bsp_tree_to_compact_format(
	submodel_bsp_tree: &bsp_builder::SubmodelBSPNode,
	materials: &MaterialsMap,
	out_map: &mut BSPMap,
	texture_name_to_index_map: &mut TextureNameToIndexMap,
) -> Submodel
{
	let polygons_before = out_map.polygons.len();
	let root_node = convert_submodel_bsp_tree_node_to_compact_format_r(
		submodel_bsp_tree,
		materials,
		out_map,
		texture_name_to_index_map,
	);
	let polygons_after = out_map.polygons.len();

	Submodel {
		root_node,
		first_polygon: polygons_before as u32,
		num_polygons: (polygons_after - polygons_before) as u32,
	}
}

fn convert_submodel_bsp_tree_node_to_compact_format_r(
	node: &bsp_builder::SubmodelBSPNode,
	materials: &MaterialsMap,
	out_map: &mut BSPMap,
	texture_name_to_index_map: &mut TextureNameToIndexMap,
) -> u32
{
	let mut children = [!0, !0];
	for i in 0 .. 2
	{
		if let Some(c) = &node.children[i]
		{
			children[i] =
				convert_submodel_bsp_tree_node_to_compact_format_r(c, materials, out_map, texture_name_to_index_map)
		}
	}

	let first_polygon = out_map.polygons.len() as u32;

	let polygons_splitted = bsp_builder::split_long_polygons(&node.polygons, materials);
	for polygon in &polygons_splitted
	{
		let polygon_converted = convert_polygon_to_compact_format(&polygon, out_map, texture_name_to_index_map);
		out_map.polygons.push(polygon_converted);
	}

	let out_node = SubmodelBSPNode {
		plane: node.plane,
		first_polygon,
		num_polygons: polygons_splitted.len() as u32,
		children,
	};

	let index = out_map.submodels_bsp_nodes.len() as u32;
	out_map.submodels_bsp_nodes.push(out_node);

	index
}

type StringsCache = std::collections::HashMap<String, StringRef>;

fn convert_entity_to_compact_format(
	entity: &map_polygonizer::Entity,
	submodel_index: u32,
	out_map: &mut BSPMap,
	strings_cache: &mut StringsCache,
) -> Entity
{
	let first_key_value_pair = out_map.key_value_pairs.len() as u32;

	for (key, value) in &entity.keys
	{
		let key_value_pair = KeyValuePair {
			key: convert_string_to_compect_format(key, out_map, strings_cache),
			value: convert_string_to_compect_format(value, out_map, strings_cache),
		};
		out_map.key_value_pairs.push(key_value_pair);
	}

	Entity {
		first_key_value_pair,
		num_key_value_pairs: entity.keys.len() as u32,
		submodel_index,
	}
}

fn convert_string_to_compect_format(s: &String, out_map: &mut BSPMap, strings_cache: &mut StringsCache) -> StringRef
{
	if let Some(prev_string) = strings_cache.get(s)
	{
		return *prev_string;
	}

	// Strings data overflow.
	if out_map.strings_data.len() > 65535
	{
		return StringRef { offset: 0, size: 0 };
	}

	let offset = out_map.strings_data.len();
	out_map.strings_data.extend_from_slice(s.as_bytes());
	let size = out_map.strings_data.len() - offset;
	let result = StringRef {
		offset: offset as u16,
		size: size as u16,
	};

	strings_cache.insert(s.clone(), result);
	result
}
