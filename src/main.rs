use std::sync::Arc;
use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    camera::{Exposure, Hdr}, //need to add gpuCulling manually?
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    feathers::FeathersPlugins,
    light::{
        atmosphere::{Falloff, PhaseFunction, ScatteringMedium, ScatteringTerm},
        Atmosphere, AtmosphereEnvironmentMapLight,
    },
    pbr::{wireframe::WireframePlugin, AtmosphereSettings, ContactShadows},
    post_process::bloom::Bloom,
    platform::collections::HashMap,
    prelude::*,
    window::{PresentMode, WindowResolution},
    winit::WinitSettings,
};
use bevy_voxel_world::prelude::*;
use noise::{HybridMulti, NoiseFn, Perlin};


// Declare materials as consts for convenience
const SNOWY_BRICK: u8 = 0;
const FULL_BRICK: u8 = 1;
const GRASS: u8 = 2;

#[derive(Resource, Clone, Default)]
struct MyWorld;

impl VoxelWorldConfig for MyWorld {
    type MaterialIndex = u8;
    type ChunkUserBundle = ();

    fn texture_index_mapper(
        &self,
    ) -> Arc<dyn Fn(Self::MaterialIndex) -> [u32; 3] + Send + Sync> {
        Arc::new(|vox_mat: u8| match vox_mat {
            SNOWY_BRICK => [0, 1, 2],
            FULL_BRICK => [2, 2, 2],
            GRASS => [3, 3, 3],
            _ => [3, 3, 3],
        })
    }

    fn voxel_texture(&self) -> Option<(String, u32)> {
        Some(("example_voxel_texture.png".into(), 4))
    }

    fn spawning_distance(&self) -> u32 {
        25
    }

    fn min_despawn_distance(&self) -> u32 {
        1
    }

    fn voxel_lookup_delegate(&self) -> VoxelLookupDelegate<Self::MaterialIndex> {
        Box::new(move |_chunk_pos, _lod, _previous| get_voxel_fn())
    }

}


fn main() {
    App::new()
        .add_plugins((DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "voxel_inyun".into(),
                resolution: WindowResolution::new(1920, 1080).with_scale_factor_override(1.0),
                present_mode: PresentMode::AutoNoVsync,
                position: WindowPosition::Centered(MonitorSelection::Primary),
                ..default()
            }),
            ..default()
        }), FreeCameraPlugin, FeathersPlugins, WireframePlugin::default(),VoxelWorldPlugin::with_config(MyWorld)))
        .insert_resource(WinitSettings::continuous())
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(StaticTransformOptimizations::Enabled)
        .add_systems(Startup,(scene.spawn(), spawn_atmosphere,create_voxel_scene))
        .add_systems(PostStartup, attach_voxel_camera)
        .run();

}

fn scene() -> impl SceneList {
    bsn_list![camera(), sun()]
}

// fn terrain() -> impl Scene {
//     bsn!{
//
//             Mesh3d(asset_value(Circle::new(40.0)))
//             MeshMaterial3d::<StandardMaterial>(asset_value(Color::WHITE))
//             Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
//     }
// }

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

fn attach_voxel_camera(
    mut commands: Commands,
    camera_query: Query<Entity, (With<Camera>, Without<VoxelWorldCamera<MyWorld>>)>,
) {
    if let Ok(entity) = camera_query.single() {
        commands.entity(entity).insert(VoxelWorldCamera::<MyWorld>::default());
    }
}
fn sun() -> impl Scene {
    bsn! {
        DirectionalLight {
            shadow_maps_enabled: true,
            contact_shadows_enabled: true,
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

fn create_voxel_scene(mut voxel_world: VoxelWorld<MyWorld>) {
    // Then we can use the `u8` consts to specify the type of voxel

    // 20 by 20 floor
    for x in -10..10 {
        for z in -10..10 {
            voxel_world.set_voxel(IVec3::new(x, -1, z), WorldVoxel::Solid(GRASS));
            // Grassy floor
        }
    }

    // Some bricks
    voxel_world.set_voxel(IVec3::new(0, 0, 0), WorldVoxel::Solid(SNOWY_BRICK));
    voxel_world.set_voxel(IVec3::new(1, 0, 0), WorldVoxel::Solid(SNOWY_BRICK));
    voxel_world.set_voxel(IVec3::new(0, 0, 1), WorldVoxel::Solid(SNOWY_BRICK));
    voxel_world.set_voxel(IVec3::new(0, 0, -1), WorldVoxel::Solid(SNOWY_BRICK));
    voxel_world.set_voxel(IVec3::new(-1, 0, 0), WorldVoxel::Solid(FULL_BRICK));
    voxel_world.set_voxel(IVec3::new(-2, 0, 0), WorldVoxel::Solid(FULL_BRICK));
    voxel_world.set_voxel(IVec3::new(-1, 1, 0), WorldVoxel::Solid(SNOWY_BRICK));
    voxel_world.set_voxel(IVec3::new(-2, 1, 0), WorldVoxel::Solid(SNOWY_BRICK));
    voxel_world.set_voxel(IVec3::new(0, 1, 0), WorldVoxel::Solid(SNOWY_BRICK));
}

fn get_voxel_fn() -> Box<dyn FnMut(IVec3, Option<WorldVoxel>) -> WorldVoxel + Send + Sync>
{
    // Set up some noise to use as the terrain height map
    let mut noise = HybridMulti::<Perlin>::new(1234);
    noise.octaves = 5;
    noise.frequency = 1.1;
    noise.lacunarity = 2.8;
    noise.persistence = 0.4;

    // We use this to cache the noise value for each y column so we only need
    // to calculate it once per x/z coordinate
    let mut cache = HashMap::<(i32, i32), f64>::new();

    // Then we return this boxed closure that captures the noise and the cache
    // This will get sent off to a separate thread for meshing by bevy_voxel_world
    Box::new(move |pos: IVec3, _previous| {
        // Sea level
        if pos.y < 1 {
            return WorldVoxel::Solid(3);
        }

        let [x, y, z] = pos.as_dvec3().to_array();

        // If y is less than the noise sample, we will set the voxel to solid
        let is_ground = y < match cache.get(&(pos.x, pos.z)) {
            Some(sample) => *sample,
            None => {
                let sample = noise.get([x / 1000.0, z / 1000.0]) * 50.0;
                cache.insert((pos.x, pos.z), sample);
                sample
            }
        };

        if is_ground {
            // Solid voxel of material type 0
            WorldVoxel::Solid(0)
        } else {
            WorldVoxel::Air
        }
    })
}
