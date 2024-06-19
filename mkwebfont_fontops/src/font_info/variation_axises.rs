use hb_subset::{
    sys::{
        hb_face_t, hb_ot_name_get_utf8, hb_ot_name_id_t,
        hb_ot_var_axis_flags_t_HB_OT_VAR_AXIS_FLAG_HIDDEN, hb_ot_var_axis_info_t,
        hb_ot_var_get_axis_count, hb_ot_var_get_axis_infos, hb_subset_input_pin_axis_location,
        hb_tag_t, HB_LANGUAGE_INVALID,
    },
    FontFace, SubsetInput,
};
use std::{ffi::c_uint, ops::RangeInclusive};

#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub enum AxisName {
    Weight,
}
impl AxisName {
    fn of(name: &str) -> Option<AxisName> {
        match name {
            x if x.eq_ignore_ascii_case("Weight") => Some(Self::Weight),
            _ => None,
        }
    }

    pub fn standard_name(&self) -> &'static str {
        match self {
            AxisName::Weight => "Weight",
        }
    }
}

#[derive(Clone, Debug)]
pub struct VariationAxis {
    pub name: String,
    pub axis: Option<AxisName>,
    pub tag: hb_tag_t,
    pub range: RangeInclusive<f32>,
    pub default: f32,
    pub is_hidden: bool,
}
impl VariationAxis {
    pub(crate) fn pin(&self, face: &mut FontFace, input: &mut SubsetInput) {
        unsafe {
            hb_subset_input_pin_axis_location(
                input.as_raw(),
                face.as_raw(),
                self.tag,
                self.default,
            );
        }
    }
}

unsafe fn load_string(face: *mut hb_face_t, name: hb_ot_name_id_t) -> String {
    let mut buf = vec![0u8; 128];
    loop {
        let mut size = buf.len() as c_uint;
        hb_ot_name_get_utf8(face, name, HB_LANGUAGE_INVALID, &mut size, buf.as_mut_ptr() as *mut _);
        if size as usize != buf.len() {
            let name = &buf[..size as usize];
            return String::from_utf8_lossy(name).to_string();
        } else {
            buf = vec![0; buf.len() * 2];
        }
    }
}
unsafe fn load_axis_info(face: *mut hb_face_t, axis: hb_ot_var_axis_info_t) -> VariationAxis {
    let mut name = load_string(face, axis.name_id);
    let axis_name = AxisName::of(&name);
    if let Some(axis) = axis_name {
        name = axis.standard_name().to_string();
    }
    VariationAxis {
        name,
        axis: axis_name,
        tag: axis.tag,
        range: axis.min_value..=axis.max_value,
        default: axis.max_value,
        is_hidden: (axis.flags & hb_ot_var_axis_flags_t_HB_OT_VAR_AXIS_FLAG_HIDDEN) != 0,
    }
}

pub fn get_variation_axises(face: &FontFace) -> Vec<VariationAxis> {
    unsafe {
        let face = face.as_raw();

        let count = hb_ot_var_get_axis_count(face) as usize;
        if count == 0 {
            Vec::new()
        } else {
            let mut data = vec![
                hb_ot_var_axis_info_t {
                    axis_index: 0,
                    tag: 0,
                    name_id: hb_ot_name_id_t(0),
                    flags: 0,
                    min_value: 0.0,
                    default_value: 0.0,
                    max_value: 0.0,
                    reserved: 0,
                };
                count
            ];

            let mut ct_var = count as c_uint;
            hb_ot_var_get_axis_infos(face, 0, &mut ct_var, data.as_mut_ptr());

            let mut vec = Vec::new();
            for axis in data {
                vec.push(load_axis_info(face, axis));
            }
            vec
        }
    }
}
