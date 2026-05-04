#![deny(dead_code)]

use std::{
    ffi::{CStr, c_char, c_void},
    ptr::null,
};

use clap_clap::ffi::{
    CLAP_PLUGIN_FACTORY_ID, CLAP_VERSION, clap_host, clap_plugin, clap_plugin_descriptor,
    clap_plugin_entry, clap_plugin_factory,
};

pub mod compressor;
pub mod delay;
pub mod drust;
pub mod eq;
pub mod imager;
pub mod maximizer;
pub mod monitoring;
pub mod reverb;
pub mod rural_modeler;
pub mod saturator;

type DescriptorFn = unsafe fn() -> *const clap_plugin_descriptor;
type CreateFn = unsafe fn(*const clap_host, *const c_char) -> *const clap_plugin;

struct PluginApi {
    descriptor: DescriptorFn,
    create: CreateFn,
}

static PLUGINS: [PluginApi; 15] = [
    PluginApi {
        descriptor: eq::parametric::clap_mono_descriptor_ptr,
        create: eq::parametric::clap_mono_create_plugin,
    },
    PluginApi {
        descriptor: eq::parametric::clap_stereo_descriptor_ptr,
        create: eq::parametric::clap_stereo_create_plugin,
    },
    PluginApi {
        descriptor: eq::graphic::clap_mono_descriptor_ptr,
        create: eq::graphic::clap_mono_create_plugin,
    },
    PluginApi {
        descriptor: eq::graphic::clap_stereo_descriptor_ptr,
        create: eq::graphic::clap_stereo_create_plugin,
    },
    PluginApi {
        descriptor: compressor::clap_mono_descriptor_ptr,
        create: compressor::clap_mono_create_plugin,
    },
    PluginApi {
        descriptor: compressor::clap_stereo_descriptor_ptr,
        create: compressor::clap_stereo_create_plugin,
    },
    PluginApi {
        descriptor: maximizer::clap_descriptor_ptr,
        create: maximizer::clap_create_plugin,
    },
    PluginApi {
        descriptor: imager::clap_descriptor_ptr,
        create: imager::clap_create_plugin,
    },
    PluginApi {
        descriptor: monitoring::clap_descriptor_ptr,
        create: monitoring::clap_create_plugin,
    },
    PluginApi {
        descriptor: saturator::clap_descriptor_ptr,
        create: saturator::clap_create_plugin,
    },
    PluginApi {
        descriptor: drust::clap_descriptor_ptr,
        create: drust::clap_create_plugin,
    },
    PluginApi {
        descriptor: rural_modeler::clap_descriptor_ptr,
        create: rural_modeler::clap_create_plugin,
    },
    PluginApi {
        descriptor: reverb::clap_descriptor_ptr,
        create: reverb::clap_create_plugin,
    },
    PluginApi {
        descriptor: delay::clap_mono_descriptor_ptr,
        create: delay::clap_mono_create_plugin,
    },
    PluginApi {
        descriptor: delay::clap_stereo_descriptor_ptr,
        create: delay::clap_stereo_create_plugin,
    },
];

unsafe extern "C-unwind" fn factory_get_plugin_count(_factory: *const clap_plugin_factory) -> u32 {
    PLUGINS.len() as u32
}

unsafe extern "C-unwind" fn factory_get_plugin_descriptor(
    _factory: *const clap_plugin_factory,
    index: u32,
) -> *const clap_plugin_descriptor {
    PLUGINS
        .get(index as usize)
        .map(|p| unsafe { (p.descriptor)() })
        .unwrap_or(null())
}

unsafe extern "C-unwind" fn factory_create_plugin(
    _factory: *const clap_plugin_factory,
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    if host.is_null() || plugin_id.is_null() {
        return null();
    }

    let requested = unsafe { CStr::from_ptr(plugin_id) };
    for plugin in &PLUGINS {
        let desc = unsafe { (plugin.descriptor)() };
        if desc.is_null() {
            continue;
        }
        let id_ptr = unsafe { (*desc).id };
        if id_ptr.is_null() {
            continue;
        }
        let this_id = unsafe { CStr::from_ptr(id_ptr) };
        if this_id == requested {
            return unsafe { (plugin.create)(host, plugin_id) };
        }
    }
    null()
}

static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(factory_get_plugin_count),
    get_plugin_descriptor: Some(factory_get_plugin_descriptor),
    create_plugin: Some(factory_create_plugin),
};

unsafe extern "C-unwind" fn entry_init(_plugin_path: *const c_char) -> bool {
    true
}

unsafe extern "C-unwind" fn entry_deinit() {}

unsafe extern "C-unwind" fn entry_get_factory(factory_id: *const c_char) -> *const c_void {
    if factory_id.is_null() {
        return null();
    }
    let factory_id = unsafe { CStr::from_ptr(factory_id) };
    if factory_id == CLAP_PLUGIN_FACTORY_ID {
        &raw const FACTORY as *const _ as *const c_void
    } else {
        null()
    }
}

#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
#[used]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(entry_get_factory),
};
