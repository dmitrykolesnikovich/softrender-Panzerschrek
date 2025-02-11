use super::frame_info::*;
use crate::common::{bbox::*, bsp_map_compact, math_types::*};
use std::sync::Arc;

pub struct InlineModelsIndex
{
	map: Arc<bsp_map_compact::BSPMap>,
	leafs_info: Vec<LeafInfo>,
	models_info: Vec<ModelInfo>,
}

#[derive(Default, Clone)]
struct LeafInfo
{
	models: Vec<u32>,
}

#[derive(Clone)]
struct ModelInfo
{
	leafs: Vec<u32>,
	bbox: BBox,
	current_entity: SubmodelEntityOpt,
}

impl InlineModelsIndex
{
	pub fn new(map: Arc<bsp_map_compact::BSPMap>) -> Self
	{
		Self {
			leafs_info: vec![LeafInfo::default(); map.leafs.len()],
			models_info: prepare_models_info(&map),
			map,
		}
	}

	pub fn position_models(&mut self, submodels: &[SubmodelEntityOpt])
	{
		debug_assert!(submodels.len() == self.models_info.len());

		for (index, submodel) in submodels.iter().enumerate()
		{
			if self.models_info[index as usize].current_entity != *submodel
			{
				self.force_reposition_model(index as u32, submodel);
			}
		}
	}

	pub fn get_leaf_models(&self, leaf_index: u32) -> &[u32]
	{
		&self.leafs_info[leaf_index as usize].models
	}

	pub fn get_model_leafs(&self, model_index: u32) -> &[u32]
	{
		&self.models_info[model_index as usize].leafs
	}

	pub fn get_model_bbox_for_ordering(&self, model_index: u32) -> BBox
	{
		// Reduce slightly bbox of inline models that is used for ordering to fix some glitches in cases with touching models.
		let mut bbox = self.models_info[model_index as usize].bbox;
		let center = bbox.get_center();
		let eps = 1.0;
		bbox.max.x = (bbox.max.x - eps).max(center.x);
		bbox.min.x = (bbox.min.x + eps).min(center.x);
		bbox.max.y = (bbox.max.y - eps).max(center.y);
		bbox.min.y = (bbox.min.y + eps).min(center.y);
		bbox.max.z = (bbox.max.z - eps).max(center.z);
		bbox.min.z = (bbox.min.z + eps).min(center.z);
		bbox
	}

	pub fn get_model_matrix(&self, model_index: u32) -> Option<Mat4f>
	{
		get_model_matrix(&self.models_info[model_index as usize])
	}

	fn force_reposition_model(&mut self, model_index: u32, submodel_opt: &SubmodelEntityOpt)
	{
		// First, erase this model index from models list of all leafs where this model was before.
		let model_info = &mut self.models_info[model_index as usize];
		for &leaf_index in &model_info.leafs
		{
			let leaf_info = &mut self.leafs_info[leaf_index as usize];
			leaf_info.models.retain(|index| *index != model_index);
		}
		// Reset model's leafs list.
		model_info.leafs.clear();

		// Set new position.
		model_info.current_entity = *submodel_opt;

		let transform_matrix = if let Some(m) = get_model_matrix(model_info)
		{
			m
		}
		else
		{
			return;
		};

		// Calculate trasformed bounding box vertices.
		let bbox_vertices = model_info
			.bbox
			.get_corners_vertices()
			.map(|v| (transform_matrix * v.extend(1.0)).truncate());

		// Place model in leafs.
		let root_node = bsp_map_compact::get_root_node_index(&self.map);
		self.position_model_r(model_index, &bbox_vertices, root_node);
	}

	// Recursively place model in leafs. Perform bounding box vertices check agains BPS node planes in order to do this.
	fn position_model_r(&mut self, model_index: u32, bbox_vertices: &[Vec3f; 8], node_index: u32)
	{
		if node_index >= bsp_map_compact::FIRST_LEAF_INDEX
		{
			let leaf_index = node_index - bsp_map_compact::FIRST_LEAF_INDEX;
			self.leafs_info[leaf_index as usize].models.push(model_index);
			self.models_info[model_index as usize].leafs.push(leaf_index);
		}
		else
		{
			let node = &self.map.nodes[node_index as usize];

			let mut vertices_front = 0;
			for &vertex in bbox_vertices
			{
				if node.plane.vec.dot(vertex) > node.plane.dist
				{
					vertices_front += 1;
				}
			}

			let node_children = node.children;

			if vertices_front > 0
			{
				self.position_model_r(model_index, bbox_vertices, node_children[0]);
			}
			if vertices_front < bbox_vertices.len()
			{
				self.position_model_r(model_index, bbox_vertices, node_children[1]);
			}
		}
	}
}

fn prepare_models_info(map: &bsp_map_compact::BSPMap) -> Vec<ModelInfo>
{
	let mut result = Vec::with_capacity(map.submodels.len());

	for submodel in &map.submodels
	{
		result.push(prepare_model_info(map, submodel));
	}

	result
}

fn prepare_model_info(map: &bsp_map_compact::BSPMap, submodel: &bsp_map_compact::Submodel) -> ModelInfo
{
	let mut bbox = bsp_map_compact::get_submodel_bbox(map, submodel);
	// Extend bounding box a bit to fix problem with missing polygons on BSP leaf edges.
	let extend_eps = 1.0 / 4.0;
	let extend_vec = Vec3f::new(extend_eps, extend_eps, extend_eps);
	bbox.max += extend_vec;
	bbox.min -= extend_vec;

	ModelInfo {
		leafs: Vec::new(),
		bbox,
		current_entity: None,
	}
}

fn get_model_matrix(model_info: &ModelInfo) -> Option<Mat4f>
{
	if let Some(e) = &model_info.current_entity
	{
		let center = model_info.bbox.get_center();
		Some(Mat4f::from_translation(e.position) * Mat4f::from(e.rotation) * Mat4f::from_translation(-center))
	}
	else
	{
		None
	}
}
