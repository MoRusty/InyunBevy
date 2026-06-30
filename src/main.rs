//! A procedurally generated city.
//!
//! This scene is intended to be an attractive, fairly realistic stress test of Bevy's capacity
//! to model extremely large scenes.
//! As a result, the complexity is higher than in most examples or benchmarks —
//! we want to use a large number of features so that pathological paths
//! are caught during development, rather than by end users.

use argh::FromArgs;
//use assets::{load_assets, CityAssets};
use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    camera::{visibility::NoCpuCulling, Exposure, Hdr},
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    color::palettes::css::WHITE,
    feathers::{dark_theme::create_dark_theme, theme::UiTheme, FeathersPlugins},
    light::{
        atmosphere::{Falloff, PhaseFunction, ScatteringMedium, ScatteringTerm},
        Atmosphere, AtmosphereEnvironmentMapLight,
    },
    pbr::{
        wireframe::{WireframeConfig, WireframePlugin},
        AtmosphereSettings, ContactShadows,
    },
    post_process::bloom::Bloom,
    prelude::*,
    window::{PresentMode, WindowResolution},
    winit::WinitSettings,
    world_serialization::WorldInstanceReady,
};
use bevy::color::palettes::css;
use bevy::ecs::system::entity_command::insert;
use bevy_cube_marcher::gpu::*;
use bevy_cube_marcher::*;




use crate::settings::{settings_ui, Settings};


#[derive(TypePath)]
struct MyComputeSampler;

impl GpuChunkComputer for MyComputeSampler {
    fn shader() -> ShaderRef {
        "sdf.wgsl".into()
    }
}


mod voxels;
mod settings;

#[derive(FromArgs, Resource, Clone)]
/// Config
pub struct Args {
    /// seed
    #[argh(option, default = "42")]
    seed: u64,

    /// size
    #[argh(option, default = "30")]
    size: u32,

    /// adds NoCpuCulling to all meshes
    #[argh(switch)]
    no_cpu_culling: bool,
}

fn main() {
    let args: Args = argh::from_env();

    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "bevy_city".into(),
                    resolution: WindowResolution::new(1920, 1080).with_scale_factor_override(1.0),
                    present_mode: PresentMode::AutoNoVsync,
                    position: WindowPosition::Centered(MonitorSelection::Primary),
                    ..default()
                }),
                ..default()
            }),
            FreeCameraPlugin,
            FeathersPlugins,
            WireframePlugin::default(),
            GpuMarchingCubesPlugin::<MyComputeSampler, (), StandardMaterial>::default(),
        ))
        .insert_resource(args.clone())
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(WinitSettings::continuous())
        .insert_resource(ChunkGeneratorSettings::<MyComputeSampler>::new(64, 8.0).with_max_chunks_per_frame(8))
        .init_resource::<Settings>()
        .insert_resource(UiTheme(create_dark_theme()))
        .insert_resource(WireframeConfig {
            global: false,
            default_color: WHITE.into(),
            ..default()
        })
        // Like in many realistic large scenes, many of the objects don't move
        // We can accelerate transform propagation by optimizing for this case
        .insert_resource(StaticTransformOptimizations::Enabled)
        .add_systems(Startup, (scene.spawn(), spawn_atmosphere, spawn_terrain))
        .add_observer(add_no_cpu_culling_on_scene_ready)
        .run();
}

fn scene() -> impl SceneList {
    bsn_list![camera(), sun()]
}


fn camera() -> impl Scene {
    bsn! {
        Camera3d
        Hdr
        template_value(Transform::from_xyz(15.0, 10.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y))
        FreeCamera
        AtmosphereSettings {
            // Reduce the default max distance in the aerial view LUT
            // to 16km to approximately fit the size of the city. This way the aerial perspective
            // gets more detail and has less banding artifacts compared to the 32km default.
            aerial_view_lut_max_distance: 1.6e4,
        }
        // The directional light illuminance used in this scene is
        // quite bright, so raising the exposure compensation helps
        // bring the scene to a nicer brightness range.
        Exposure::OVERCAST
        // Bloom gives the sun a much more natural look.
        Bloom::NATURAL
        // Enables the atmosphere to drive reflections and ambient lighting (IBL) for this view
        AtmosphereEnvironmentMapLight
        Msaa::Off
        TemporalAntiAliasing
        ContactShadows
    }
}


fn sun() -> impl Scene {
    bsn! {
        DirectionalLight {
            shadow_maps_enabled: {Settings::default().shadow_maps_enabled},
            contact_shadows_enabled: {Settings::default().contact_shadows_enabled},
            illuminance: light_consts::lux::RAW_SUNLIGHT,
        }
        template_value(Transform::from_xyz(1.0, 0.15, 1.0).looking_at(Vec3::ZERO, Vec3::Y))
    }
}

/// Spawns the earth atmosphere plus an extra near-ground fog term.
fn spawn_atmosphere(
    mut commands: Commands,
    mut scattering_mediums: ResMut<Assets<ScatteringMedium>>,
) {
    let mut earth_medium = ScatteringMedium::default();

    // Same 60 km atmosphere height as `ScatteringMedium::earth`
    const ATMOSPHERE_REF_HEIGHT_KM: f32 = 60.0;

    // The scale height of haze is set to 100 meters providing a low-lying dense fog layer.
    const HAZE_SCALE_HEIGHT_KM: f32 = 0.1;

    // Fog has high albedo and very low absorption resulting in a white color.
    const HAZE_SINGLE_SCATTER_ALBEDO: f32 = 0.99;

    // Distance at which contrast falls low enough to be indistinguishable from the sky.
    // known as Meteorological Optical Range
    const HAZE_VISIBILITY_KM: f32 = 12.0;

    // Koschmieder relation to calculate the extinction coefficient for the medium in m^-1 units.
    let beta_ext = (3.912 / HAZE_VISIBILITY_KM) * 1e-3;

    // Add the fog to the earth medium as an additional scattering term.
    earth_medium.terms.push(ScatteringTerm {
        absorption: Vec3::splat(beta_ext * (1.0 - HAZE_SINGLE_SCATTER_ALBEDO)),
        scattering: Vec3::splat(beta_ext * HAZE_SINGLE_SCATTER_ALBEDO),
        falloff: Falloff::Exponential {
            scale: HAZE_SCALE_HEIGHT_KM / ATMOSPHERE_REF_HEIGHT_KM,
        },
        // Fog is approximated as a mie scatterer with this asymmetry factor
        phase: PhaseFunction::Mie { asymmetry: 0.76 },
    });
    let earth_atmosphere = Atmosphere::earth(scattering_mediums.add(earth_medium));

    // This scale means that 1 city block in this scene will be roughly 100 meters relative to the atmosphere.
    let scale = 1.0 / 20.0;
    commands.spawn((
        earth_atmosphere.clone(),
        Transform::from_scale(Vec3::splat(scale))
            .with_translation(-Vec3::Y * earth_atmosphere.inner_radius * scale),
    ));
}


/// Adds [`NoCpuCulling`] to all meshes in the scene after the city is done spawning
fn add_no_cpu_culling(
    mut commands: Commands,
    meshes: Query<Entity, (With<Mesh3d>, Without<NoCpuCulling>)>,
    args: Res<Args>,
) {
    if args.no_cpu_culling {
        for entity in meshes.iter() {
            commands.entity(entity).insert(NoCpuCulling);
        }
    }
}

/// Adds [`NoCpuCulling`] to all meshes in all scenes after the city is done spawning
///
/// This is required because a few assets are spawned using a [`WorldAssetRoot`] instead of directly
/// spawning a [`Mesh`]
fn add_no_cpu_culling_on_scene_ready(
    scene_ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    meshes: Query<(), (With<Mesh3d>, Without<NoCpuCulling>)>,
    args: Res<Args>,
) {
    if args.no_cpu_culling {
        for descendant in children.iter_descendants(scene_ready.entity) {
            if meshes.get(descendant).is_ok() {
                commands.entity(descendant).insert(NoCpuCulling);
            }
        }
    }
}

fn spawn_terrain(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.spawn(ChunkLoader::<MyComputeSampler>::new(8));

    commands.insert_resource(ChunkMaterial::<MyComputeSampler, StandardMaterial>::new(
        materials.add(Color::from(css::DARK_GREEN)),
    ));
}
