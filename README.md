# Inyun Bevy

[![License](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](https://github.com/bevyengine/bevy#license)

## What is Inyun Bevy

Inyun Bevy is a voxel terrain engine built on top of the Bevy game engine. This will be the core engine behind my game Inyun. More info coming soon!


## Current State
Using bevy_voxel_world to create the voxel terrain
Using the BSN macro feature from bevy 0.20

![bvw_480](https://github.com/MoRusty/InyunBevy/assets/screenshots/sample1.png)
![bvw_480](https://github.com/MoRusty/InyunBevy/assets/screenshots/sample2.png)

## Next goal
- decide on an Isosurface Extraction Algorithm, currently deciding between Marching Cubes or Dual Contouring.
- Implement the chosen algorithm and integrate it with the voxel terrain system.
- Optimize the voxel terrain system for performance and memory usage.
- implement a first person camera mode with an entity that can WASD move around the terrain