//! Block appearance: flat colours + simplified geometry (boxes in 1/16 block units).

use crate::formats::{is_air_name, BlockState};
use crate::legacy::COLORS;

pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug)]
pub struct Bx {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub color: Option<Rgb>,
}

fn bx(x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32) -> Bx {
    Bx { min: [x0, y0, z0], max: [x1, y1, z1], color: None }
}
fn bxc(x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32, c: Rgb) -> Bx {
    Bx { min: [x0, y0, z0], max: [x1, y1, z1], color: Some(c) }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Air,
    Full,
    Boxes,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conn {
    Fence,
    Pane,
    Wall,
}

/// Face order: 0 -X (west), 1 +X (east), 2 -Y (down), 3 +Y (up), 4 -Z (north), 5 +Z (south).
pub struct Info {
    pub kind: Kind,
    pub boxes: Vec<Bx>,
    pub color: Rgb,
    pub face_colors: Option<[Rgb; 6]>,
    pub alpha: u8,
    /// Fence/pane/wall without connection properties: connect to neighbours at mesh time.
    pub auto_conn: Option<Conn>,
    pub conn: Option<Conn>,
}

impl Info {
    #[inline]
    pub fn occludes(&self) -> bool {
        self.kind == Kind::Full && self.alpha == 255
    }
}

// ---------------------------------------------------------------- colours

fn dye(c: &str) -> Option<Rgb> {
    Some(match c {
        "white" => [234, 236, 237],
        "orange" => [241, 118, 20],
        "magenta" => [190, 69, 180],
        "light_blue" => [58, 175, 217],
        "yellow" => [249, 198, 40],
        "lime" => [112, 185, 26],
        "pink" => [238, 141, 172],
        "gray" => [63, 68, 72],
        "light_gray" => [142, 142, 135],
        "cyan" => [21, 138, 145],
        "purple" => [122, 42, 173],
        "blue" => [53, 57, 157],
        "brown" => [114, 72, 41],
        "green" => [85, 110, 28],
        "red" => [161, 39, 35],
        "black" => [21, 21, 26],
        _ => return None,
    })
}
fn concrete(c: &str) -> Option<Rgb> {
    Some(match c {
        "white" => [207, 213, 214],
        "orange" => [224, 97, 1],
        "magenta" => [169, 48, 159],
        "light_blue" => [36, 137, 199],
        "yellow" => [241, 175, 21],
        "lime" => [94, 169, 24],
        "pink" => [214, 101, 143],
        "gray" => [55, 58, 62],
        "light_gray" => [125, 125, 115],
        "cyan" => [21, 119, 136],
        "purple" => [100, 32, 156],
        "blue" => [45, 47, 143],
        "brown" => [96, 60, 32],
        "green" => [73, 91, 36],
        "red" => [142, 33, 33],
        "black" => [8, 10, 15],
        _ => return None,
    })
}
fn terracotta(c: &str) -> Option<Rgb> {
    Some(match c {
        "white" => [210, 178, 161],
        "orange" => [162, 84, 38],
        "magenta" => [150, 88, 109],
        "light_blue" => [113, 109, 138],
        "yellow" => [186, 133, 35],
        "lime" => [104, 118, 53],
        "pink" => [162, 78, 79],
        "gray" => [58, 42, 36],
        "light_gray" => [135, 107, 98],
        "cyan" => [87, 91, 91],
        "purple" => [118, 70, 86],
        "blue" => [74, 60, 91],
        "brown" => [77, 51, 36],
        "green" => [76, 83, 42],
        "red" => [143, 61, 47],
        "black" => [37, 23, 16],
        _ => return None,
    })
}
fn planks(w: &str) -> Option<Rgb> {
    Some(match w {
        "oak" => [162, 131, 79],
        "spruce" => [115, 85, 49],
        "birch" => [192, 175, 121],
        "jungle" => [160, 115, 81],
        "acacia" => [168, 90, 50],
        "dark_oak" => [67, 43, 20],
        "mangrove" => [118, 54, 49],
        "cherry" => [226, 178, 172],
        "bamboo" => [194, 173, 80],
        "crimson" => [101, 49, 71],
        "warped" => [43, 105, 99],
        "pale_oak" => [228, 218, 216],
        _ => return None,
    })
}
fn bark(w: &str) -> Option<Rgb> {
    Some(match w {
        "oak" => [109, 85, 51],
        "spruce" => [59, 38, 17],
        "birch" => [216, 215, 210],
        "jungle" => [85, 67, 25],
        "acacia" => [103, 96, 86],
        "dark_oak" => [60, 47, 26],
        "mangrove" => [84, 67, 41],
        "cherry" => [55, 33, 44],
        "crimson" => [92, 25, 29],
        "warped" => [58, 58, 77],
        "pale_oak" => [88, 77, 73],
        _ => return None,
    })
}
fn leaves(w: &str) -> Rgb {
    match w {
        "spruce" => [61, 98, 61],
        "birch" => [110, 145, 70],
        "cherry" => [229, 172, 196],
        "azalea" | "flowering_azalea" => [95, 125, 45],
        "pale_oak" => [160, 165, 150],
        "mangrove" => [88, 130, 38],
        "dark_oak" | "jungle" | "acacia" => [70, 120, 35],
        _ => [72, 126, 42],
    }
}

const WOODS: [&str; 12] =
    ["dark_oak", "pale_oak", "oak", "spruce", "birch", "jungle", "acacia", "mangrove", "cherry", "bamboo", "crimson", "warped"];

fn exact(n: &str) -> Option<(Rgb, u8)> {
    let o = |c: Rgb| Some((c, 255));
    match n {
        "stone" | "infested_stone" | "stone_bricks_dummy" => o([125, 125, 125]),
        "cobblestone" | "infested_cobblestone" => o([118, 118, 118]),
        "mossy_cobblestone" => o([110, 118, 94]),
        "smooth_stone" => o([158, 158, 158]),
        "stone_bricks" | "infested_stone_bricks" | "chiseled_stone_bricks" => o([122, 121, 122]),
        "cracked_stone_bricks" => o([118, 117, 118]),
        "mossy_stone_bricks" => o([115, 121, 105]),
        "granite" => o([149, 103, 85]),
        "polished_granite" => o([154, 106, 89]),
        "diorite" => o([188, 188, 188]),
        "polished_diorite" => o([192, 193, 194]),
        "andesite" => o([136, 136, 136]),
        "polished_andesite" => o([132, 134, 133]),
        "deepslate" | "cobbled_deepslate" | "chiseled_deepslate" => o([80, 80, 82]),
        "polished_deepslate" => o([72, 72, 73]),
        "deepslate_bricks" | "cracked_deepslate_bricks" => o([70, 70, 71]),
        "deepslate_tiles" | "cracked_deepslate_tiles" => o([54, 54, 55]),
        "reinforced_deepslate" => o([80, 83, 79]),
        "tuff" | "polished_tuff" | "tuff_bricks" | "chiseled_tuff" => o([108, 109, 102]),
        "calcite" => o([223, 224, 220]),
        "dripstone_block" | "pointed_dripstone" => o([134, 107, 92]),
        "dirt" | "farmland" => o([134, 96, 67]),
        "coarse_dirt" => o([119, 85, 59]),
        "rooted_dirt" => o([144, 103, 76]),
        "dirt_path" => o([148, 121, 65]),
        "mud" => o([60, 57, 60]),
        "packed_mud" => o([142, 106, 79]),
        "mud_bricks" => o([137, 103, 79]),
        "clay" => o([160, 166, 179]),
        "gravel" | "suspicious_gravel" => o([131, 127, 126]),
        "sand" | "suspicious_sand" => o([219, 207, 163]),
        "red_sand" => o([190, 102, 33]),
        "sandstone" | "chiseled_sandstone" | "cut_sandstone" | "smooth_sandstone" => o([216, 203, 155]),
        "red_sandstone" | "chiseled_red_sandstone" | "cut_red_sandstone" | "smooth_red_sandstone" => o([186, 99, 29]),
        "bedrock" => o([85, 85, 85]),
        "obsidian" => o([15, 10, 24]),
        "crying_obsidian" => o([32, 10, 60]),
        "netherrack" => o([97, 38, 38]),
        "nether_bricks" | "cracked_nether_bricks" | "chiseled_nether_bricks" | "nether_brick" => o([44, 21, 26]),
        "red_nether_bricks" | "red_nether_brick" => o([69, 7, 9]),
        "soul_sand" => o([81, 62, 50]),
        "soul_soil" => o([75, 57, 46]),
        "basalt" | "polished_basalt" | "smooth_basalt" => o([80, 81, 86]),
        "blackstone" | "polished_blackstone" | "chiseled_polished_blackstone" | "gilded_blackstone" => o([42, 36, 41]),
        "polished_blackstone_bricks" | "cracked_polished_blackstone_bricks" | "polished_blackstone_brick" => o([48, 42, 49]),
        "glowstone" => o([171, 131, 84]),
        "shroomlight" => o([240, 146, 70]),
        "magma_block" => o([142, 63, 31]),
        "nether_wart_block" => o([114, 2, 2]),
        "warped_wart_block" => o([22, 119, 121]),
        "crimson_nylium" => o([130, 31, 31]),
        "warped_nylium" => o([43, 114, 101]),
        "end_stone" => o([219, 222, 158]),
        "end_stone_bricks" | "end_stone_brick" => o([218, 224, 162]),
        "purpur_block" | "purpur_pillar" | "purpur" => o([169, 125, 169]),
        "prismarine" => o([99, 156, 151]),
        "prismarine_bricks" | "prismarine_brick" => o([99, 171, 158]),
        "dark_prismarine" => o([51, 91, 75]),
        "sea_lantern" => o([172, 199, 190]),
        "quartz_block" | "smooth_quartz" | "quartz_pillar" | "chiseled_quartz_block" | "quartz_bricks" | "quartz" => {
            o([235, 229, 222])
        }
        "bricks" | "brick" => o([150, 97, 83]),
        "bone_block" => o([229, 225, 207]),
        "terracotta" => o([152, 94, 68]),
        "snow" | "snow_block" | "powder_snow" => o([249, 254, 254]),
        "ice" | "frosted_ice" => Some(([145, 183, 253], 170)),
        "packed_ice" => o([141, 180, 250]),
        "blue_ice" => o([116, 167, 253]),
        "water" | "bubble_column" => Some(([52, 92, 220], 140)),
        "lava" => o([207, 92, 15]),
        "glass" | "glass_pane" => Some(([200, 222, 230], 80)),
        "tinted_glass" => Some(([44, 38, 46], 200)),
        "slime_block" => Some(([115, 192, 91], 200)),
        "honey_block" => Some(([251, 185, 52], 200)),
        "cobweb" => Some(([220, 220, 220], 140)),
        "grass_block" => o([134, 96, 67]),
        "mycelium" => o([111, 99, 105]),
        "podzol" => o([91, 63, 24]),
        "moss_block" | "moss_carpet" => o([89, 109, 45]),
        "pale_moss_block" | "pale_moss_carpet" => o([107, 112, 104]),
        "coal_ore" | "deepslate_coal_ore" => o([105, 105, 105]),
        "iron_ore" | "deepslate_iron_ore" => o([136, 129, 122]),
        "copper_ore" | "deepslate_copper_ore" => o([124, 125, 120]),
        "gold_ore" | "deepslate_gold_ore" | "nether_gold_ore" => o([145, 133, 106]),
        "redstone_ore" | "deepslate_redstone_ore" => o([133, 107, 107]),
        "lapis_ore" | "deepslate_lapis_ore" => o([99, 110, 132]),
        "diamond_ore" | "deepslate_diamond_ore" => o([121, 141, 140]),
        "emerald_ore" | "deepslate_emerald_ore" => o([117, 136, 124]),
        "nether_quartz_ore" => o([117, 65, 62]),
        "ancient_debris" => o([94, 66, 58]),
        "iron_block" | "heavy_weighted_pressure_plate" | "iron_door" | "iron_trapdoor" => o([220, 220, 220]),
        "gold_block" | "light_weighted_pressure_plate" => o([246, 208, 61]),
        "diamond_block" => o([98, 237, 228]),
        "emerald_block" => o([42, 203, 87]),
        "lapis_block" => o([30, 67, 140]),
        "redstone_block" => o([175, 24, 5]),
        "coal_block" => o([16, 15, 15]),
        "netherite_block" => o([66, 61, 63]),
        "raw_iron_block" => o([166, 135, 107]),
        "raw_copper_block" => o([154, 105, 79]),
        "raw_gold_block" => o([221, 169, 46]),
        "amethyst_block" | "budding_amethyst" | "amethyst_cluster" => o([133, 97, 191]),
        "iron_bars" => o([150, 150, 150]),
        "chain" | "iron_chain" => o([60, 64, 75]),
        "piston" | "sticky_piston" => o([110, 108, 103]),
        "piston_head" => o([153, 127, 85]),
        "observer" => o([98, 98, 98]),
        "dispenser" | "dropper" => o([122, 122, 122]),
        "hopper" => o([70, 70, 72]),
        "redstone_wire" => o([170, 10, 5]),
        "redstone_torch" | "redstone_wall_torch" => o([220, 40, 20]),
        "torch" | "wall_torch" => o([240, 200, 110]),
        "soul_torch" | "soul_wall_torch" => o([110, 220, 230]),
        "redstone_lamp" => o([150, 90, 50]),
        "repeater" | "comparator" => o([160, 160, 160]),
        "target" => o([226, 170, 157]),
        "lever" => o([110, 90, 60]),
        "note_block" | "jukebox" => o([88, 58, 40]),
        "tnt" => o([219, 68, 52]),
        "daylight_detector" => o([130, 116, 94]),
        "tripwire" => o([200, 200, 200]),
        "tripwire_hook" => o([140, 130, 120]),
        "lightning_rod" => o([192, 107, 79]),
        "end_rod" => o([230, 225, 215]),
        "sculk_sensor" | "calibrated_sculk_sensor" | "sculk" | "sculk_catalyst" | "sculk_shrieker" | "sculk_vein" => {
            o([12, 40, 50])
        }
        "crafter" => o([110, 100, 95]),
        "bookshelf" | "chiseled_bookshelf" => o([117, 94, 59]),
        "crafting_table" => o([120, 73, 42]),
        "furnace" | "blast_furnace" | "smoker" => o([110, 110, 110]),
        "chest" | "trapped_chest" | "barrel" => o([162, 112, 40]),
        "ender_chest" => o([40, 50, 52]),
        "shulker_box" => o([139, 96, 139]),
        "beacon" => o([117, 220, 215]),
        "sponge" => o([195, 192, 74]),
        "wet_sponge" => o([171, 181, 70]),
        "hay_block" => o([166, 136, 38]),
        "pumpkin" | "carved_pumpkin" | "jack_o_lantern" => o([198, 118, 24]),
        "melon" => o([111, 145, 30]),
        "cactus" => o([85, 127, 43]),
        "sugar_cane" => o([148, 192, 101]),
        "bamboo" => o([93, 144, 19]),
        "scaffolding" => o([170, 131, 73]),
        "ladder" => o([124, 98, 58]),
        "vine" | "glow_lichen" | "lily_pad" => o([60, 110, 30]),
        "anvil" | "chipped_anvil" | "damaged_anvil" => o([68, 68, 68]),
        "enchanting_table" => o([60, 30, 40]),
        "cauldron" | "water_cauldron" | "lava_cauldron" | "powder_snow_cauldron" => o([60, 60, 62]),
        "brewing_stand" => o([120, 110, 90]),
        "bell" => o([230, 190, 60]),
        "lantern" => o([90, 80, 70]),
        "soul_lantern" => o([70, 90, 100]),
        "campfire" | "soul_campfire" => o([110, 80, 50]),
        "flower_pot" => o([124, 68, 52]),
        "cake" => o([238, 230, 220]),
        "spawner" | "trial_spawner" | "vault" => o([36, 46, 62]),
        "dragon_egg" => o([12, 9, 15]),
        "end_portal_frame" => o([90, 120, 100]),
        "end_portal" | "end_gateway" | "nether_portal" => Some(([90, 40, 160], 150)),
        "command_block" => o([180, 136, 108]),
        "chain_command_block" => o([132, 165, 150]),
        "repeating_command_block" => o([129, 110, 176]),
        "structure_block" | "jigsaw" => o([88, 74, 90]),
        "barrier" => Some(([230, 40, 40], 60)),
        "honeycomb_block" => o([229, 148, 29]),
        "beehive" | "bee_nest" => o([180, 146, 90]),
        "lodestone" => o([147, 149, 152]),
        "respawn_anchor" => o([40, 20, 70]),
        "conduit" => o([160, 140, 110]),
        "copper_block" | "cut_copper" | "chiseled_copper" | "copper_grate" | "copper_door" | "copper_trapdoor" | "copper_bulb" => {
            o([192, 107, 79])
        }
        "exposed_copper" | "exposed_cut_copper" | "exposed_chiseled_copper" | "exposed_copper_grate" | "exposed_copper_bulb" => {
            o([161, 125, 103])
        }
        "weathered_copper" | "weathered_cut_copper" | "weathered_chiseled_copper" | "weathered_copper_grate" | "weathered_copper_bulb" => {
            o([108, 153, 110])
        }
        "oxidized_copper" | "oxidized_cut_copper" | "oxidized_chiseled_copper" | "oxidized_copper_grate" | "oxidized_copper_bulb" => {
            o([82, 162, 132])
        }
        "dandelion" | "sunflower" => o([240, 210, 40]),
        "poppy" | "rose_bush" | "red_tulip" | "red_mushroom" => o([200, 30, 30]),
        "blue_orchid" | "cornflower" => o([60, 120, 230]),
        "allium" | "lilac" => o([180, 120, 220]),
        "azure_bluet" | "oxeye_daisy" | "white_tulip" | "lily_of_the_valley" => o([225, 230, 230]),
        "orange_tulip" | "torchflower" => o([235, 130, 40]),
        "pink_tulip" | "peony" | "pink_petals" => o([230, 160, 200]),
        "brown_mushroom" => o([150, 112, 82]),
        "brown_mushroom_block" => o([149, 111, 81]),
        "red_mushroom_block" => o([200, 46, 45]),
        "mushroom_stem" => o([203, 196, 185]),
        "dead_bush" => o([120, 85, 40]),
        "kelp" | "kelp_plant" | "seagrass" | "tall_seagrass" => o([60, 130, 40]),
        "wheat" | "carrots" | "potatoes" | "beetroots" => o([90, 150, 40]),
        "chorus_plant" | "chorus_flower" => o([95, 60, 95]),
        "sweet_berry_bush" => o([60, 100, 40]),
        "fire" | "soul_fire" => Some(([240, 140, 30], 200)),
        "rail" | "powered_rail" | "detector_rail" | "activator_rail" => o([130, 120, 105]),
        "composter" => o([120, 80, 40]),
        "lectern" => o([170, 130, 80]),
        "cocoa" => o([150, 90, 40]),
        "weeping_vines" | "weeping_vines_plant" => o([150, 20, 20]),
        "twisting_vines" | "twisting_vines_plant" => o([20, 140, 130]),
        "smithing_table" => o([60, 60, 70]),
        "sea_pickle" => o([100, 110, 40]),
        "loom" => o([160, 130, 100]),
        "azalea" | "flowering_azalea" | "azalea_bush" | "flowering_azalea_bush" => o([100, 125, 50]),
        "cartography_table" => o([100, 80, 60]),
        "fletching_table" => o([190, 170, 120]),
        "dried_kelp_block" => o([50, 60, 40]),
        "turtle_egg" => o([220, 220, 190]),
        "candle" => o([230, 220, 190]),
        "spore_blossom" => o([210, 100, 150]),
        "pumpkin_stem" | "melon_stem" | "attached_pumpkin_stem" | "attached_melon_stem" | "small_dripleaf" | "big_dripleaf"
        | "big_dripleaf_stem" => o([100, 145, 40]),
        "hanging_roots" => o([160, 110, 85]),
        "wither_rose" => o([40, 35, 30]),
        "skeleton_skull" | "skeleton_wall_skull" => o([200, 200, 200]),
        "wither_skeleton_skull" | "wither_skeleton_wall_skull" | "dragon_head" | "dragon_wall_head" => o([40, 40, 40]),
        "zombie_head" | "zombie_wall_head" => o([80, 130, 70]),
        "creeper_head" | "creeper_wall_head" => o([90, 170, 80]),
        "player_head" | "player_wall_head" => o([110, 80, 60]),
        "piglin_head" | "piglin_wall_head" => o([200, 140, 120]),
        "small_amethyst_bud" | "medium_amethyst_bud" | "large_amethyst_bud" => o([160, 120, 210]),
        _ => None,
    }
}

fn hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

const SUFFIXES: [&str; 18] = [
    "_stairs", "_slab", "_wall", "_fence_gate", "_fence", "_pressure_plate", "_button", "_trapdoor", "_door", "_wall_hanging_sign",
    "_hanging_sign", "_wall_sign", "_sign", "_pane", "_carpet", "_shelf", "_grate", "_bulb",
];

/// Colour + alpha for a block name.
pub fn color_of(n: &str) -> (Rgb, u8) {
    let n = n.strip_prefix("waxed_").unwrap_or(n);
    if let Some(c) = exact(n) {
        return c;
    }
    // Dyed families.
    for c in COLORS.iter().rev() {
        if let Some(rest) = n.strip_prefix(c).and_then(|r| r.strip_prefix('_')) {
            let d = dye(c).unwrap();
            return match rest {
                "concrete" => (concrete(c).unwrap(), 255),
                "concrete_powder" => {
                    let k = concrete(c).unwrap();
                    ([k[0].saturating_add(18), k[1].saturating_add(18), k[2].saturating_add(18)], 255)
                }
                "terracotta" | "glazed_terracotta" => (terracotta(c).unwrap(), 255),
                "stained_glass" | "stained_glass_pane" => (d, 120),
                _ => (d, 255),
            };
        }
    }
    // Wood families.
    for w in WOODS {
        if let Some(rest) = n.strip_prefix(w).and_then(|r| r.strip_prefix('_')) {
            let rest = rest.trim_start_matches("wall_");
            return match rest {
                "log" | "wood" | "stem" | "hyphae" => (bark(w).unwrap_or([100, 80, 50]), 255),
                "leaves" => (leaves(w), 255),
                "sapling" | "propagule" => ([80, 130, 50], 255),
                _ => (planks(w).unwrap_or([150, 120, 80]), 255),
            };
        }
    }
    if let Some(w) = n.strip_prefix("stripped_") {
        for wd in WOODS {
            if w.starts_with(wd) {
                return (planks(wd).unwrap_or([150, 120, 80]), 255);
            }
        }
    }
    if n.ends_with("_leaves") {
        return (leaves(n.trim_end_matches("_leaves")), 255);
    }
    if n.starts_with("potted_") {
        return ([124, 68, 52], 255);
    }
    if let Some(r) = n.strip_prefix("infested_") {
        return color_of(r);
    }
    // Derived shapes: oak_stairs -> oak, stone_brick_wall -> stone_bricks, ...
    for suf in SUFFIXES {
        if let Some(base) = n.strip_suffix(suf) {
            for cand in [base.to_string(), format!("{base}s"), format!("{base}_block"), format!("{base}_planks")] {
                if let Some(c) = exact(&cand) {
                    return c;
                }
            }
            let (c, a) = color_of_fallback(base);
            return (c, if suf == "_pane" { a.min(120) } else { a });
        }
    }
    color_of_fallback(n)
}

pub fn color_of_fallback(n: &str) -> (Rgb, u8) {
    if let Some(c) = exact(n) {
        return c;
    }
    let kw: [(&str, Rgb); 20] = [
        ("glass", [200, 222, 230]),
        ("deepslate", [70, 70, 72]),
        ("blackstone", [42, 36, 41]),
        ("sandstone", [216, 203, 155]),
        ("quartz", [235, 229, 222]),
        ("prismarine", [99, 156, 151]),
        ("purpur", [169, 125, 169]),
        ("nether", [44, 21, 26]),
        ("end_stone", [219, 222, 158]),
        ("copper", [192, 107, 79]),
        ("brick", [150, 97, 83]),
        ("tuff", [108, 109, 102]),
        ("cobble", [118, 118, 118]),
        ("stone", [125, 125, 125]),
        ("mud", [137, 103, 79]),
        ("iron", [210, 210, 210]),
        ("gold", [246, 208, 61]),
        ("coral", [200, 80, 120]),
        ("grass", [96, 150, 55]),
        ("fern", [90, 140, 55]),
    ];
    for (k, c) in kw {
        if n.contains(k) {
            return (c, if k == "glass" { 90 } else { 255 });
        }
    }
    if std::env::var_os("SCHEMVIEW_UNKNOWN").is_some() {
        eprintln!("UNKNOWN {n}");
    }
    let h = hash(n);
    // Muted, deterministic fallback colour for unknown / modded blocks.
    let c = [(90 + (h & 0x5f)) as u8, (90 + ((h >> 8) & 0x5f)) as u8, (90 + ((h >> 16) & 0x5f)) as u8];
    (c, 255)
}

// ---------------------------------------------------------------- geometry helpers

/// Rotate a box defined facing north (front = -Z) to the given horizontal facing.
fn rot_h(b: Bx, facing: &str) -> Bx {
    let f = |x: f32, z: f32| -> (f32, f32) {
        match facing {
            "south" => (16.0 - x, 16.0 - z),
            "east" => (16.0 - z, x),
            "west" => (z, 16.0 - x),
            _ => (x, z),
        }
    };
    let (ax, az) = f(b.min[0], b.min[2]);
    let (bx_, bz) = f(b.max[0], b.max[2]);
    Bx { min: [ax.min(bx_), b.min[1], az.min(bz)], max: [ax.max(bx_), b.max[1], az.max(bz)], color: b.color }
}

/// Map a box defined facing up (front = +Y) to any of the 6 directions.
fn rot6(b: Bx, facing: &str) -> Bx {
    let f = |p: [f32; 3]| -> [f32; 3] {
        let [x, y, z] = p;
        match facing {
            "down" => [x, 16.0 - y, z],
            "north" => [x, z, 16.0 - y],
            "south" => [x, z, y],
            "east" => [y, x, z],
            "west" => [16.0 - y, x, z],
            _ => [x, y, z],
        }
    };
    let a = f(b.min);
    let c = f(b.max);
    Bx {
        min: [a[0].min(c[0]), a[1].min(c[1]), a[2].min(c[2])],
        max: [a[0].max(c[0]), a[1].max(c[1]), a[2].max(c[2])],
        color: b.color,
    }
}

fn face_index(dir: &str) -> Option<usize> {
    Some(match dir {
        "west" => 0,
        "east" => 1,
        "down" => 2,
        "up" => 3,
        "north" => 4,
        "south" => 5,
        _ => return None,
    })
}
fn opposite(dir: &str) -> &'static str {
    match dir {
        "west" => "east",
        "east" => "west",
        "down" => "up",
        "up" => "down",
        "north" => "south",
        _ => "north",
    }
}

fn cross_plant(h: f32) -> Vec<Bx> {
    vec![bx(2.0, 0.0, 7.5, 14.0, h, 8.5), bx(7.5, 0.0, 2.0, 8.5, h, 14.0)]
}

pub fn conn_boxes(c: Conn, n: bool, e: bool, s: bool, w: bool, wall_tall: [bool; 4]) -> Vec<Bx> {
    let mut v = vec![];
    let dirs = [(n, "north"), (e, "east"), (s, "south"), (w, "west")];
    match c {
        Conn::Fence => {
            v.push(bx(6.0, 0.0, 6.0, 10.0, 16.0, 10.0));
            for (on, d) in dirs {
                if on {
                    v.push(rot_h(bx(7.0, 12.0, 0.0, 9.0, 15.0, 6.0), d));
                    v.push(rot_h(bx(7.0, 6.0, 0.0, 9.0, 9.0, 6.0), d));
                }
            }
        }
        Conn::Pane => {
            v.push(bx(7.0, 0.0, 7.0, 9.0, 16.0, 9.0));
            for (on, d) in dirs {
                if on {
                    v.push(rot_h(bx(7.0, 0.0, 0.0, 9.0, 16.0, 7.0), d));
                }
            }
        }
        Conn::Wall => {
            v.push(bx(4.0, 0.0, 4.0, 12.0, 16.0, 12.0));
            for (i, (on, d)) in dirs.into_iter().enumerate() {
                if on {
                    let h = if wall_tall[i] { 16.0 } else { 14.0 };
                    v.push(rot_h(bx(5.0, 0.0, 0.0, 11.0, h, 4.0), d));
                }
            }
        }
    }
    v
}

fn is_true(s: Option<&str>) -> bool {
    matches!(s, Some("true"))
}

fn wall_like_name(n: &str) -> bool {
    n.ends_with("_wall")
        && !n.ends_with("_banner")
        && !n.contains("wall_torch")
        && !n.contains("wall_sign")
        && !n.contains("wall_head")
        && !n.contains("wall_skull")
        && !n.contains("wall_fan")
}

// ---------------------------------------------------------------- shapes

pub fn info_for(st: &BlockState) -> Info {
    let n = st.name.as_str();
    let p = |k: &str| st.prop(k);
    let mut info = Info {
        kind: Kind::Full,
        boxes: vec![],
        color: [0; 3],
        face_colors: None,
        alpha: 255,
        auto_conn: None,
        conn: None,
    };
    if is_air_name(n) {
        info.kind = Kind::Air;
        return info;
    }
    let (color, alpha) = color_of(n);
    info.color = color;
    info.alpha = alpha;
    let base = n.strip_prefix("waxed_").unwrap_or(n);

    let facing = p("facing").unwrap_or("north");
    let mut boxes: Vec<Bx> = vec![];
    let full = |info: &mut Info| info.kind = Kind::Full;

    if base.ends_with("_slab") {
        match p("type") {
            Some("double") => {}
            Some("top") => boxes.push(bx(0.0, 8.0, 0.0, 16.0, 16.0, 16.0)),
            _ => boxes.push(bx(0.0, 0.0, 0.0, 16.0, 8.0, 16.0)),
        }
    } else if base.ends_with("_stairs") {
        let top = p("half") == Some("top");
        let (sy0, sy1, qy0, qy1) = if top { (8.0, 16.0, 0.0, 8.0) } else { (0.0, 8.0, 8.0, 16.0) };
        boxes.push(bx(0.0, sy0, 0.0, 16.0, sy1, 16.0));
        let bl = bx(0.0, qy0, 0.0, 8.0, qy1, 8.0);
        let br = bx(8.0, qy0, 0.0, 16.0, qy1, 8.0);
        let fl = bx(0.0, qy0, 8.0, 8.0, qy1, 16.0);
        let fr = bx(8.0, qy0, 8.0, 16.0, qy1, 16.0);
        let qs: Vec<Bx> = match p("shape") {
            Some("outer_left") => vec![bl],
            Some("outer_right") => vec![br],
            Some("inner_left") => vec![bl, br, fl],
            Some("inner_right") => vec![bl, br, fr],
            _ => vec![bx(0.0, qy0, 0.0, 16.0, qy1, 8.0)],
        };
        for q in qs {
            boxes.push(rot_h(q, facing));
        }
    } else if base == "piston" || base == "sticky_piston" {
        let front = if base == "sticky_piston" { [118, 168, 86] } else { [153, 127, 85] };
        if is_true(p("extended")) {
            boxes.push(rot6(bx(0.0, 0.0, 0.0, 16.0, 12.0, 16.0), facing));
            // Base front (where the arm comes out) is dark.
            boxes.push(rot6(bxc(0.0, 11.9, 0.0, 16.0, 12.0, 16.0, [70, 68, 64]), facing));
        } else {
            let mut fc = [color; 6];
            if let Some(i) = face_index(facing) {
                fc[i] = front;
            }
            if let Some(i) = face_index(opposite(facing)) {
                fc[i] = [85, 83, 79];
            }
            info.face_colors = Some(fc);
        }
    } else if base == "piston_head" {
        let plate = if p("type") == Some("sticky") { [118, 168, 86] } else { [153, 127, 85] };
        boxes.push(rot6(bxc(0.0, 12.0, 0.0, 16.0, 16.0, 16.0, plate), facing));
        let short = is_true(p("short"));
        boxes.push(rot6(bxc(6.0, if short { 0.0 } else { -4.0 }, 6.0, 10.0, 12.0, 10.0, [153, 127, 85]), facing));
    } else if matches!(base, "observer" | "dispenser" | "dropper" | "crafter") {
        let mut fc = [color; 6];
        if let Some(i) = face_index(facing) {
            fc[i] = [62, 62, 64];
        }
        if base == "observer" {
            if let Some(i) = face_index(opposite(facing)) {
                fc[i] = if is_true(p("powered")) { [220, 40, 30] } else { [120, 82, 80] };
            }
        }
        info.face_colors = Some(fc);
    } else if base == "redstone_wire" {
        let power: f32 = p("power").and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let t = power / 15.0;
        let c = [(110.0 + 145.0 * t) as u8, (5.0 + 35.0 * t) as u8, (2.0 + 18.0 * t) as u8];
        info.color = c;
        let sides = ["north", "east", "south", "west"];
        let vals: Vec<Option<&str>> = sides.iter().map(|s| p(s)).collect();
        let any_prop = vals.iter().any(|v| v.is_some());
        let all_none = vals.iter().all(|v| matches!(v, Some("none")));
        boxes.push(bx(5.0, 0.0, 5.0, 11.0, 1.0, 11.0));
        for (i, d) in sides.iter().enumerate() {
            let on = if !any_prop { true } else { !matches!(vals[i], Some("none") | None) };
            if on && !all_none {
                boxes.push(rot_h(bx(6.0, 0.0, 0.0, 10.0, 1.0, 5.0), d));
                if vals[i] == Some("up") {
                    boxes.push(rot_h(bx(6.0, 0.0, 0.0, 10.0, 16.0, 1.0), d));
                }
            }
        }
    } else if base == "repeater" || base == "comparator" {
        boxes.push(bx(0.0, 0.0, 0.0, 16.0, 2.0, 16.0));
        let lit = is_true(p("powered"));
        let tc = if lit { [255, 50, 25] } else { [120, 40, 30] };
        if base == "repeater" {
            let delay: f32 = p("delay").and_then(|s| s.parse().ok()).unwrap_or(1.0);
            // Facing side is the input; output is opposite (+Z when facing north).
            boxes.push(rot_h(bxc(7.0, 2.0, 12.0, 9.0, 7.0, 14.0, tc), facing));
            let z = 8.0 - 2.0 * (delay - 1.0);
            boxes.push(rot_h(bxc(7.0, 2.0, z - 2.0, 9.0, 7.0, z, tc), facing));
        } else {
            let sub = p("mode") == Some("subtract");
            boxes.push(rot_h(bxc(3.0, 2.0, 2.0, 5.0, 7.0, 4.0, tc), facing));
            boxes.push(rot_h(bxc(11.0, 2.0, 2.0, 13.0, 7.0, 4.0, tc), facing));
            let fc = if sub { [255, 50, 25] } else { [120, 40, 30] };
            boxes.push(rot_h(bxc(7.0, 2.0, 11.0, 9.0, 5.0, 13.0, fc), facing));
        }
    } else if matches!(base, "torch" | "redstone_torch" | "soul_torch") {
        let c = if base == "redstone_torch" && p("lit") == Some("false") { [110, 30, 20] } else { color };
        boxes.push(bxc(7.0, 0.0, 7.0, 9.0, 8.0, 9.0, [120, 92, 55]));
        boxes.push(bxc(7.0, 8.0, 7.0, 9.0, 10.0, 9.0, c));
    } else if matches!(base, "wall_torch" | "redstone_wall_torch" | "soul_wall_torch") {
        let c = if base == "redstone_wall_torch" && p("lit") == Some("false") { [110, 30, 20] } else { color };
        boxes.push(rot_h(bxc(7.0, 3.0, 12.0, 9.0, 11.0, 15.0, [120, 92, 55]), facing));
        boxes.push(rot_h(bxc(7.0, 11.0, 11.5, 9.0, 13.0, 13.5, c), facing));
    } else if base == "lever" {
        let face = p("face").unwrap_or("floor");
        let up = is_true(p("powered"));
        let (b0, s0) = (bxc(5.0, 0.0, 4.0, 11.0, 3.0, 12.0, [120, 120, 120]), bx(7.0, 3.0, 7.0, 9.0, 11.0, 9.0));
        let s0 = if up { Bx { min: [7.0, 3.0, 4.0], max: [9.0, 10.0, 6.0], ..s0 } } else { Bx { min: [7.0, 3.0, 10.0], max: [9.0, 10.0, 12.0], ..s0 } };
        for b in [b0, s0] {
            let b = match face {
                "ceiling" => rot6(rot_h(b, facing), "down"),
                "wall" => rot_h(rot6(b, "south"), facing),
                _ => rot_h(b, facing),
            };
            boxes.push(b);
        }
    } else if base.ends_with("_button") {
        let face = p("face").unwrap_or("wall");
        let b = bx(5.0, 0.0, 6.0, 11.0, 2.0, 10.0);
        boxes.push(match face {
            "floor" => rot_h(b, facing),
            "ceiling" => rot6(rot_h(b, facing), "down"),
            _ => rot_h(bx(5.0, 6.0, 14.0, 11.0, 10.0, 16.0), facing),
        });
    } else if base.ends_with("_pressure_plate") {
        boxes.push(bx(1.0, 0.0, 1.0, 15.0, 1.0, 15.0));
    } else if base.ends_with("_carpet") || base == "moss_carpet" || base == "pale_moss_carpet" {
        boxes.push(bx(0.0, 0.0, 0.0, 16.0, 1.0, 16.0));
    } else if base == "snow" {
        let l: f32 = p("layers").and_then(|s| s.parse().ok()).unwrap_or(1.0);
        if l < 8.0 {
            boxes.push(bx(0.0, 0.0, 0.0, 16.0, 2.0 * l.max(1.0), 16.0));
        }
    } else if base.ends_with("rail") {
        info.color = [130, 120, 105];
        boxes.push(bx(0.0, 0.0, 0.0, 16.0, 1.0, 16.0));
    } else if base.ends_with("_fence") {
        info.conn = Some(Conn::Fence);
        if p("north").is_none() {
            info.auto_conn = Some(Conn::Fence);
        } else {
            boxes = conn_boxes(Conn::Fence, is_true(p("north")), is_true(p("east")), is_true(p("south")), is_true(p("west")), [false; 4]);
        }
    } else if base.ends_with("_pane") || base == "iron_bars" || base == "glass_pane" {
        info.conn = Some(Conn::Pane);
        if p("north").is_none() {
            info.auto_conn = Some(Conn::Pane);
        } else {
            boxes = conn_boxes(Conn::Pane, is_true(p("north")), is_true(p("east")), is_true(p("south")), is_true(p("west")), [false; 4]);
        }
    } else if wall_like_name(base) {
        info.conn = Some(Conn::Wall);
        if p("north").is_none() {
            info.auto_conn = Some(Conn::Wall);
        } else {
            let on = |k: &str| matches!(p(k), Some("low") | Some("tall") | Some("true"));
            let tall = |k: &str| p(k) == Some("tall");
            boxes = conn_boxes(
                Conn::Wall,
                on("north"),
                on("east"),
                on("south"),
                on("west"),
                [tall("north"), tall("east"), tall("south"), tall("west")],
            );
            if p("up") == Some("false") {
                boxes.remove(0);
            }
        }
    } else if base.ends_with("_fence_gate") {
        if is_true(p("open")) {
            boxes.push(rot_h(bx(0.0, 5.0, 7.0, 2.0, 16.0, 9.0), facing));
            boxes.push(rot_h(bx(14.0, 5.0, 7.0, 16.0, 16.0, 9.0), facing));
            boxes.push(rot_h(bx(0.0, 6.0, 9.0, 2.0, 15.0, 16.0), facing));
            boxes.push(rot_h(bx(14.0, 6.0, 9.0, 16.0, 15.0, 16.0), facing));
        } else {
            boxes.push(rot_h(bx(0.0, 5.0, 7.0, 2.0, 16.0, 9.0), facing));
            boxes.push(rot_h(bx(14.0, 5.0, 7.0, 16.0, 16.0, 9.0), facing));
            boxes.push(rot_h(bx(2.0, 6.0, 7.0, 14.0, 9.0, 9.0), facing));
            boxes.push(rot_h(bx(2.0, 12.0, 7.0, 14.0, 15.0, 9.0), facing));
        }
    } else if base.ends_with("_door") {
        let open = is_true(p("open"));
        let right = p("hinge") == Some("right");
        let b = if !open {
            bx(0.0, 0.0, 13.0, 16.0, 16.0, 16.0)
        } else if right {
            bx(13.0, 0.0, 0.0, 16.0, 16.0, 16.0)
        } else {
            bx(0.0, 0.0, 0.0, 3.0, 16.0, 16.0)
        };
        boxes.push(rot_h(b, facing));
    } else if base.ends_with("_trapdoor") {
        if is_true(p("open")) {
            boxes.push(rot_h(bx(0.0, 0.0, 13.0, 16.0, 16.0, 16.0), facing));
        } else if p("half") == Some("top") {
            boxes.push(bx(0.0, 13.0, 0.0, 16.0, 16.0, 16.0));
        } else {
            boxes.push(bx(0.0, 0.0, 0.0, 16.0, 3.0, 16.0));
        }
    } else if base == "chain" || base == "iron_chain" || base.ends_with("_chain") {
        let b = bx(6.5, 0.0, 6.5, 9.5, 16.0, 9.5);
        boxes.push(match p("axis") {
            Some("x") => rot6(b, "east"),
            Some("z") => rot6(b, "south"),
            _ => b,
        });
    } else if base == "end_rod" || base == "lightning_rod" {
        boxes.push(rot6(bx(6.0, 0.0, 6.0, 10.0, 16.0, 10.0), p("facing").unwrap_or("up")));
    } else if base == "ladder" || base == "vine" || base == "glow_lichen" || base == "sculk_vein" {
        if base == "ladder" {
            boxes.push(rot_h(bx(0.0, 0.0, 15.0, 16.0, 16.0, 16.0), facing));
        } else {
            let mut any = false;
            for d in ["north", "south", "east", "west"] {
                if is_true(p(d)) {
                    any = true;
                    boxes.push(rot_h(bx(0.0, 0.0, 0.0, 16.0, 16.0, 1.0), d));
                }
            }
            if is_true(p("up")) {
                any = true;
                boxes.push(bx(0.0, 15.0, 0.0, 16.0, 16.0, 16.0));
            }
            if is_true(p("down")) {
                any = true;
                boxes.push(bx(0.0, 0.0, 0.0, 16.0, 1.0, 16.0));
            }
            if !any {
                boxes.push(bx(0.0, 0.0, 15.0, 16.0, 16.0, 16.0));
            }
        }
    } else if base.ends_with("_wall_sign") {
        boxes.push(rot_h(bx(0.0, 4.5, 14.0, 16.0, 12.5, 16.0), facing));
    } else if base.ends_with("_wall_hanging_sign") || base.ends_with("_hanging_sign") {
        boxes.push(bx(1.0, 0.0, 7.0, 15.0, 10.0, 9.0));
    } else if base.ends_with("_sign") {
        boxes.push(bx(7.0, 0.0, 7.0, 9.0, 9.0, 9.0));
        boxes.push(bx(0.0, 9.0, 7.0, 16.0, 16.0, 9.0));
    } else if base.ends_with("_wall_banner") {
        boxes.push(rot_h(bx(1.0, -14.0, 14.0, 15.0, 13.0, 16.0), facing));
    } else if base.ends_with("_banner") {
        boxes.push(bx(7.0, 0.0, 7.0, 9.0, 16.0, 9.0));
        boxes.push(bx(1.0, 2.0, 7.0, 15.0, 16.0, 8.0));
    } else if base.ends_with("_bed") {
        boxes.push(bx(0.0, 3.0, 0.0, 16.0, 9.0, 16.0));
        for (x, z) in [(0.0, 0.0), (13.0, 0.0), (0.0, 13.0), (13.0, 13.0)] {
            boxes.push(bxc(x, 0.0, z, x + 3.0, 3.0, z + 3.0, [150, 115, 70]));
        }
    } else if matches!(base, "chest" | "trapped_chest" | "ender_chest") {
        boxes.push(bx(1.0, 0.0, 1.0, 15.0, 14.0, 15.0));
    } else if base == "hopper" {
        boxes.push(bx(0.0, 10.0, 0.0, 16.0, 16.0, 16.0));
        boxes.push(bx(4.0, 4.0, 4.0, 12.0, 10.0, 12.0));
        let f = p("facing").unwrap_or("down");
        boxes.push(if f == "down" { bx(6.0, 0.0, 6.0, 10.0, 4.0, 10.0) } else { rot_h(bx(6.0, 4.0, 0.0, 10.0, 8.0, 4.0), f) });
    } else if base == "daylight_detector" {
        boxes.push(bx(0.0, 0.0, 0.0, 16.0, 6.0, 16.0));
    } else if matches!(base, "enchanting_table") {
        boxes.push(bx(0.0, 0.0, 0.0, 16.0, 12.0, 16.0));
    } else if base == "stonecutter" {
        boxes.push(bx(0.0, 0.0, 0.0, 16.0, 9.0, 16.0));
    } else if matches!(base, "farmland" | "dirt_path") {
        boxes.push(bx(0.0, 0.0, 0.0, 16.0, 15.0, 16.0));
    } else if base == "end_portal_frame" {
        boxes.push(bx(0.0, 0.0, 0.0, 16.0, 13.0, 16.0));
    } else if base == "cactus" {
        boxes.push(bx(1.0, 0.0, 1.0, 15.0, 16.0, 15.0));
    } else if base == "cake" || base.ends_with("_cake") {
        boxes.push(bx(1.0, 0.0, 1.0, 15.0, 8.0, 15.0));
    } else if base == "flower_pot" || base.starts_with("potted_") {
        boxes.push(bx(5.0, 0.0, 5.0, 11.0, 6.0, 11.0));
        if base.starts_with("potted_") {
            let (pc, _) = color_of(&base["potted_".len()..]);
            boxes.push(bxc(7.0, 6.0, 7.0, 9.0, 12.0, 9.0, pc));
        }
    } else if base == "lantern" || base == "soul_lantern" {
        boxes.push(bx(5.0, 0.0, 5.0, 11.0, 7.0, 11.0));
        boxes.push(bx(6.0, 7.0, 6.0, 10.0, 9.0, 10.0));
    } else if base.ends_with("candle") {
        boxes.push(bx(7.0, 0.0, 7.0, 9.0, 6.0, 9.0));
    } else if base.ends_with("_wall_head") || base.ends_with("_wall_skull") {
        boxes.push(rot_h(bx(4.0, 4.0, 8.0, 12.0, 12.0, 16.0), facing));
    } else if base.ends_with("_head") || base.ends_with("_skull") {
        boxes.push(bx(4.0, 0.0, 4.0, 12.0, 8.0, 12.0));
    } else if base == "campfire" || base == "soul_campfire" {
        boxes.push(bx(0.0, 0.0, 0.0, 16.0, 7.0, 16.0));
    } else if base == "cauldron" || base.ends_with("_cauldron") {
        boxes.push(bx(0.0, 3.0, 0.0, 16.0, 4.0, 16.0));
        boxes.push(bx(0.0, 0.0, 0.0, 2.0, 16.0, 16.0));
        boxes.push(bx(14.0, 0.0, 0.0, 16.0, 16.0, 16.0));
        boxes.push(bx(2.0, 0.0, 0.0, 14.0, 16.0, 2.0));
        boxes.push(bx(2.0, 0.0, 14.0, 14.0, 16.0, 16.0));
    } else if base == "brewing_stand" {
        boxes.push(bx(7.0, 0.0, 7.0, 9.0, 14.0, 9.0));
        boxes.push(bx(1.0, 0.0, 1.0, 15.0, 2.0, 15.0));
    } else if base == "tripwire" {
        boxes.push(bx(0.0, 1.0, 7.5, 16.0, 1.5, 8.5));
        boxes.push(bx(7.5, 1.0, 0.0, 8.5, 1.5, 16.0));
    } else if base == "tripwire_hook" {
        boxes.push(rot_h(bx(6.0, 1.0, 14.0, 10.0, 15.0, 16.0), facing));
    } else if base == "lily_pad" {
        boxes.push(bx(1.0, 0.0, 1.0, 15.0, 0.5, 15.0));
    } else if base == "bamboo" {
        boxes.push(bx(6.5, 0.0, 6.5, 9.5, 16.0, 9.5));
    } else if base == "anvil" || base == "chipped_anvil" || base == "damaged_anvil" {
        boxes.push(bx(2.0, 0.0, 2.0, 14.0, 4.0, 14.0));
        boxes.push(bx(4.0, 4.0, 3.0, 12.0, 10.0, 13.0));
        boxes.push(bx(0.0, 10.0, 3.0, 16.0, 16.0, 13.0));
    } else if base == "bell" {
        boxes.push(bx(4.0, 3.0, 4.0, 12.0, 12.0, 12.0));
    } else if base == "sculk_sensor" || base == "calibrated_sculk_sensor" {
        boxes.push(bx(0.0, 0.0, 0.0, 16.0, 8.0, 16.0));
    } else if base == "pointed_dripstone" || base == "amethyst_cluster" || base.ends_with("_bud") {
        boxes.push(bx(5.0, 0.0, 5.0, 11.0, 12.0, 11.0));
    } else if base == "cobweb" {
        boxes = cross_plant(16.0);
        boxes.push(bx(0.0, 7.5, 2.0, 16.0, 8.5, 14.0));
    } else if base == "fire" || base == "soul_fire" {
        boxes = cross_plant(14.0);
    } else if base == "sugar_cane" || base == "kelp_plant" || base == "kelp" || base == "tall_seagrass" || base == "twisting_vines_plant" || base == "weeping_vines_plant" {
        boxes = cross_plant(16.0);
    } else if is_plant(base) {
        boxes = cross_plant(12.0);
    } else if base == "grass_block" {
        let mut fc = [color; 6];
        fc[3] = [106, 160, 62];
        info.face_colors = Some(fc);
    } else if base == "podzol" || base == "mycelium" {
        let mut fc = [[134, 96, 67]; 6];
        fc[3] = color;
        info.face_colors = Some(fc);
    } else if base.ends_with("_log") || base.ends_with("_stem") && !base.contains("mushroom") && !base.contains("melon") && !base.contains("pumpkin") {
        // Show end-grain on the log's axis faces.
        let inner = if base.starts_with("stripped_") {
            [color[0].saturating_sub(14), color[1].saturating_sub(14), color[2].saturating_sub(14)]
        } else {
            color_of(&format!("stripped_{base}")).0
        };
        let mut fc = [color; 6];
        let (a, b) = match p("axis").unwrap_or("y") {
            "x" => (0, 1),
            "z" => (4, 5),
            _ => (2, 3),
        };
        fc[a] = inner;
        fc[b] = inner;
        info.face_colors = Some(fc);
    }

    if !boxes.is_empty() {
        info.kind = Kind::Boxes;
        info.boxes = boxes;
    } else if info.auto_conn.is_some() {
        info.kind = Kind::Boxes;
    } else if info.kind != Kind::Air {
        // Things that produced no boxes but must not be full blocks.
        if matches!(base, "snow") {
            full(&mut info);
        }
    }
    info
}

fn is_plant(n: &str) -> bool {
    n.ends_with("_sapling")
        || n.ends_with("_tulip")
        || n.ends_with("_mushroom")
        || n.ends_with("_fungus")
        || n.ends_with("_roots")
        || n.ends_with("_propagule")
        || n.ends_with("_coral")
        || n.ends_with("_coral_fan")
        || n.ends_with("_wall_fan")
        || n.ends_with("_sprouts")
        || matches!(
            n,
            "short_grass" | "grass" | "fern" | "tall_grass" | "large_fern" | "dead_bush" | "dandelion" | "poppy" | "blue_orchid"
                | "allium" | "azure_bluet" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley" | "wither_rose" | "sunflower"
                | "lilac" | "rose_bush" | "peony" | "seagrass" | "wheat" | "carrots" | "potatoes" | "beetroots"
                | "sweet_berry_bush" | "nether_wart" | "torchflower" | "pitcher_plant" | "pink_petals" | "cocoa"
                | "melon_stem" | "pumpkin_stem" | "attached_melon_stem" | "attached_pumpkin_stem" | "hanging_roots"
                | "spore_blossom" | "small_dripleaf" | "big_dripleaf" | "big_dripleaf_stem" | "glow_berries"
                | "cave_vines" | "cave_vines_plant" | "twisting_vines" | "weeping_vines" | "bush" | "firefly_bush"
                | "short_dry_grass" | "tall_dry_grass" | "leaf_litter" | "wildflowers" | "eyeblossom" | "open_eyeblossom"
                | "closed_eyeblossom" | "frogspawn" | "sea_pickle"
        )
}
