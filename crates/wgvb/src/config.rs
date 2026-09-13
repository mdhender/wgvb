//! Generator configuration and its validation.
//!
//! See `DESIGN.md` sections 19.1, 21, 21.1, and 21.2.
//!
//! Configuration is immutable after construction and is authoritative world
//! data: the complete effective configuration, including every value that came
//! from a default, is written when a world is created. Reopening a world never
//! substitutes current program defaults for missing stored values.

use crate::{DEFAULT_CHUNK_SIZE_HEXES, DEFAULT_MACRO_REGION_SIZE_HEXES, DEFAULT_REGION_SIZE_HEXES};

/// Why a configuration was rejected.
///
/// One variant per failure class, so tests assert on variants instead of
/// matching message strings. See `DESIGN.md` section 19.1.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("{field} must be finite, got {value}")]
    NotFinite { field: &'static str, value: f64 },
    #[error("{field} must be positive, got {value}")]
    NotPositive { field: &'static str, value: f64 },
    #[error("{field} must be in [{lo}, {hi}], got {value}")]
    OutOfRange {
        field: &'static str,
        value: f64,
        lo: f64,
        hi: f64,
    },
    #[error("{field} must be in 1..=16, got {octaves}")]
    OctaveCount { field: &'static str, octaves: u8 },
    #[error(
        "{field} = {octaves} puts an octave at {finest_wavelength_miles} miles, \
         below the {limit_miles}-mile Nyquist wavelength of the tile grid"
    )]
    BelowNyquist {
        field: &'static str,
        octaves: u8,
        finest_wavelength_miles: f64,
        limit_miles: f64,
    },
    #[error("elevation contrast passes must be in 0..=4, got {0}")]
    PassCount(u8),
    #[error("{field} ({value}) must be greater than {below} ({limit})")]
    NotAscending {
        field: &'static str,
        value: f64,
        below: &'static str,
        limit: f64,
    },
}

/// Largest accepted fbm octave count. More octaves than this cannot add detail
/// at any wavelength the world can express, and each one costs a sample.
pub const MAX_FBM_OCTAVES: u8 = 16;

/// Largest accepted elevation shaping pass count.
///
/// Four passes at full contrast steepen the middle of the scale by more than
/// five, which already leaves almost every tile at one extreme or the other.
/// More would be a step function with extra arithmetic.
pub const MAX_CONTRAST_PASSES: u8 = 4;

/// Immutable generator configuration.
///
/// Every value that can alter generated output lives here rather than as a
/// constant scattered through the implementation, so a world file records the
/// whole of what produced it. Scale fields name their unit in the identifier.
///
/// `deny_unknown_fields` is required, not stylistic: an older binary reading a
/// newer world file must reject it rather than silently ignore the fields it
/// does not understand. For the same reason no field carries
/// `#[serde(default)]` — a defaulted missing field would be a changed world with
/// an unchanged version number, which is precisely what the versioning scheme
/// exists to prevent. Adding a field here is an algorithm version change.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Elevation threshold separating ocean water from potential land, on the
    /// normalized `[-1, +1]` elevation scale.
    ///
    /// One of the six [`crate::Elevation`] band thresholds, which must ascend
    /// strictly: `deep_water_level < sea_level < upland_level <
    /// highland_level < mountain_level`. See `DESIGN.md` sections 14.1 and 15.
    ///
    /// Sea level is a *threshold on a globally stable field*, never a quantile
    /// of a generated sample. Section 33.5: computing it from the explored area
    /// would make exploration order change the world. If a target land fraction
    /// is wanted, move this value and re-measure, which is what the
    /// distribution tests in `tests/elevation.rs` are for.
    pub sea_level: f64,

    /// Below this elevation a water tile is [`crate::Elevation::DeepWater`].
    pub deep_water_level: f64,
    /// Above [`Config::sea_level`] and up to this, land is
    /// [`crate::Elevation::Lowland`].
    pub upland_level: f64,
    /// Up to this, land is [`crate::Elevation::Upland`].
    pub highland_level: f64,
    /// Up to this, land is [`crate::Elevation::Highland`]; above it,
    /// [`crate::Elevation::Mountain`].
    pub mountain_level: f64,

    /// Wavelength of the broad continental land/ocean field.
    pub continental_wavelength_miles: f64,
    /// Wavelength of regional uplift.
    pub regional_wavelength_miles: f64,
    /// Wavelength of hill-scale relief.
    pub local_wavelength_miles: f64,
    /// Wavelength of the finest terrain detail.
    pub detail_wavelength_miles: f64,

    /// Wavelength of the broad heat field of `DESIGN.md` section 16.
    ///
    /// Longer than [`Config::continental_wavelength_miles`] on purpose. The
    /// wrapped world has no equator, so heat zones are procedural rather than
    /// latitudinal, and a climate band shorter than a continent would put a
    /// desert and an icecap on the same island. At two thousand hexes a
    /// continent spans about half a zone.
    pub heat_wavelength_miles: f64,
    /// Wavelength of the broad moisture field.
    ///
    /// Shorter than the heat field: rainfall varies over a continent in a way
    /// temperature does not, and the two axes are meant to read as independent
    /// rather than as one field and its shadow.
    pub moisture_wavelength_miles: f64,
    /// Wavelength of the local variation term of the moisture composite.
    ///
    /// The `local_variation` of section 16's moisture formula. Heat has no
    /// counterpart, which is section 16 written literally rather than an
    /// omission.
    pub moisture_variation_wavelength_miles: f64,

    /// Wavelength of the ridge structure field of `DESIGN.md` section 10.
    ///
    /// The *crests* this produces are the zero crossings of that field, so they
    /// are spaced about half a wavelength apart rather than one.
    pub ridge_wavelength_miles: f64,
    /// How far along a ridge line the structure field is averaged, in miles.
    ///
    /// Ridges are elongated by averaging the field at three points spaced this
    /// far apart along the region's ridge orientation, which stretches features
    /// along the ridge and leaves them sharp across it. Zero would leave the
    /// field isotropic and the ridge orientation unused.
    pub ridge_elongation_miles: f64,

    /// Wavelength of the domain-warp offset field.
    pub warp_wavelength_miles: f64,
    /// Magnitude of the domain-warp offset. To disable warping, compose a field
    /// graph without a warp stage rather than setting this to zero.
    pub warp_strength_miles: f64,

    /// Wavelength of the weaker, higher-frequency warp that breaks up local
    /// relief. `DESIGN.md` section 13 calls for low-frequency warps for large
    /// geography and weaker high-frequency warps for local irregularity, which
    /// is two warp scales, not one.
    pub detail_warp_wavelength_miles: f64,
    /// Magnitude of the high-frequency warp offset.
    pub detail_warp_strength_miles: f64,

    /// Octave count for the continentalness ladder, in `1..=16`.
    ///
    /// One count per field rather than one for all five. The field graph
    /// already carries the octave count per [`crate::Field::Fbm`] node, so a
    /// single shared setting was an artificial coupling in one closure, and it
    /// forced the two shortest fields to run octaves below what the tile grid
    /// can carry. See [`crate::NYQUIST_WAVELENGTH_MILES`], which
    /// [`Config::validate`] enforces as a ceiling on every ladder.
    ///
    /// Nyquist is a bound, not the mechanism that picks these numbers. Deriving
    /// each count by truncating at the limit would make it a step function of a
    /// float — retuning a wavelength by a tenth of a mile would silently flip a
    /// count and move every value in the world, which is the knife-edge
    /// threshold section 25.6 warns about.
    pub continental_octaves: u8,
    /// Octave count for the regional uplift ladder, in `1..=16`.
    pub regional_octaves: u8,
    /// Octave count for the ridge structure ladder, in `1..=16`.
    pub ridge_octaves: u8,
    /// Octave count for the hill-scale ladder, in `1..=16`.
    pub local_octaves: u8,
    /// Octave count for the finest detail ladder, in `1..=16`.
    pub detail_octaves: u8,
    /// Octave count for the broad heat ladder, in `1..=16`.
    pub heat_octaves: u8,
    /// Octave count for the broad moisture ladder, in `1..=16`.
    pub moisture_octaves: u8,
    /// Octave count for the moisture variation ladder, in `1..=16`.
    pub moisture_variation_octaves: u8,
    /// Frequency multiplier between successive fbm octaves.
    ///
    /// Shared by every ladder: lacunarity is shape rather than scale, and
    /// nothing about the five fields suggests they want different ones.
    pub fbm_lacunarity: f64,
    /// Amplitude multiplier between successive fbm octaves.
    pub fbm_gain: f64,

    /// Weight of continentalness in the multi-scale composite of `DESIGN.md`
    /// section 10. The composite divides by the total weight, so these are
    /// relative amplitudes rather than absolute ones.
    pub continental_weight: f64,
    /// Weight of regional uplift in the multi-scale composite.
    pub regional_weight: f64,
    /// Weight of hill-scale relief in the multi-scale composite.
    pub local_weight: f64,
    /// Weight of the finest terrain detail in the multi-scale composite.
    pub detail_weight: f64,
    /// Weight of the blended regional elevation bias in the elevation scalar.
    ///
    /// This is the `regional_uplift` term of `DESIGN.md` section 10, and it is
    /// the only term that comes from the region hierarchy rather than from a
    /// continuous noise field.
    pub uplift_weight: f64,
    /// Weight of the ridge structure term in the elevation scalar.
    pub ridge_weight: f64,

    /// Weight of the broad heat field in the temperature composite of
    /// `DESIGN.md` section 16.
    ///
    /// The composite divides by the total weight, so this and
    /// [`Config::heat_region_weight`] are relative amplitudes. Elevation
    /// cooling is not one of them; it is subtracted afterwards, which is what
    /// section 16's formula says and is why a mountain is colder than the plain
    /// it stands on rather than merely closer to the regional average.
    pub heat_field_weight: f64,
    /// Weight of the blended region heat bias in the temperature composite.
    pub heat_region_weight: f64,
    /// Weight of the broad moisture field in the moisture composite.
    pub moisture_field_weight: f64,
    /// Weight of the blended region moisture bias in the moisture composite.
    pub moisture_region_weight: f64,
    /// Weight of the local variation term in the moisture composite.
    pub moisture_variation_weight: f64,

    /// How much temperature the full height of the land scale costs, in
    /// `[0, 2]`.
    ///
    /// The `elevation_cooling` of section 16. Cooling is proportional to height
    /// *above sea level*, so it is zero everywhere on water — an ocean is as
    /// warm as its latitude, not as cold as its depth — and reaches this value
    /// at an elevation of `+1`.
    ///
    /// Two is the largest that can matter: temperature is in `[-1, +1]`, so a
    /// cooling of two takes the warmest possible summit to the bottom of the
    /// scale. Zero disables it, which is the configuration the tests use to
    /// show what cooling is responsible for.
    pub elevation_cooling: f64,

    /// Upper bound of [`crate::HeatBand::Polar`], on the normalized `[-1, +1]`
    /// temperature scale.
    ///
    /// One of the four heat thresholds, which must ascend strictly:
    /// `polar_level < cold_level < temperate_level < warm_level`. Above
    /// `warm_level` a tile is [`crate::HeatBand::Hot`]. Measured against
    /// `tests/climate.rs` rather than guessed, for the same reason
    /// [`Config::elevation_offset`] was.
    pub polar_level: f64,
    /// Upper bound of [`crate::HeatBand::Cold`].
    pub cold_level: f64,
    /// Upper bound of [`crate::HeatBand::Temperate`].
    pub temperate_level: f64,
    /// Upper bound of [`crate::HeatBand::Warm`]; above it,
    /// [`crate::HeatBand::Hot`].
    pub warm_level: f64,

    /// Upper bound of [`crate::MoistureBand::Arid`], on the normalized
    /// `[-1, +1]` moisture scale.
    ///
    /// One of the four moisture thresholds, which must ascend strictly:
    /// `arid_level < dry_level < moderate_level < humid_level`. Above
    /// `humid_level` a tile is [`crate::MoistureBand::Saturated`].
    pub arid_level: f64,
    /// Upper bound of [`crate::MoistureBand::Dry`].
    pub dry_level: f64,
    /// Upper bound of [`crate::MoistureBand::Moderate`].
    pub moderate_level: f64,
    /// Upper bound of [`crate::MoistureBand::Humid`]; above it,
    /// [`crate::MoistureBand::Saturated`].
    pub humid_level: f64,

    /// Constant added to the elevation composite before it is shaped.
    ///
    /// This is the land-fraction knob, and it is why [`Config::sea_level`] can
    /// stay at zero where `DESIGN.md` section 14's scale says sea level is.
    /// A weighted average of zero-mean fields puts about half the world above
    /// zero; a negative offset sinks the world until the fraction above sea
    /// level is the intended one.
    ///
    /// Section 15 allows the land fraction to be tuned by "the continentalness
    /// distribution and the sea-level threshold", and section 33.5 forbids
    /// deriving either from a generated sample. This is the first of those two:
    /// a fixed shift of the distribution, measured once against the
    /// distribution tests and then stored, never recomputed at run time.
    pub elevation_offset: f64,

    /// How hard one shaping pass pushes the elevation composite away from sea
    /// level, in `[0, 1]`.
    ///
    /// A weighted average of fields that are each in `[-1, +1]` clusters near
    /// zero: with the alpha defaults, ninety per cent of the composite lies
    /// within a quarter of the scale, which would leave a world with no deep
    /// ocean and no mountains. One pass is the odd polynomial
    /// `x + contrast * (x - x^3) / 2`, which is monotone on `[-1, +1]` for any
    /// contrast in `[0, 1]`, fixes both ends exactly, and steepens the middle
    /// by `1 + contrast / 2`. A polynomial rather than an exponent because
    /// section 25.2 bars `powf`.
    ///
    /// Zero disables the shaping. One is the steepest a single pass can be and
    /// stay monotone — above it the map would fold two elevations onto one — so
    /// validation rejects more, and [`Config::elevation_contrast_passes`] is
    /// how the shaping is made stronger than that.
    pub elevation_contrast: f64,

    /// How many times the shaping pass is applied, in `0..=4`.
    ///
    /// A monotone map composed with itself is monotone, and each of these
    /// passes fixes `-1`, `0`, and `+1`, so the composition does too. Two
    /// passes at full contrast steepen the middle by `2.25`, which is what the
    /// alpha defaults need for the top and bottom of the scale to be reachable
    /// at all.
    ///
    /// Paired with [`Config::elevation_contrast`] rather than replacing it: the
    /// pass count chooses the order of magnitude and the contrast trims within
    /// it, which together cover the range continuously. Zero of either disables
    /// the shaping.
    pub elevation_contrast_passes: u8,

    /// The per-hex elevation step that reads as maximum local relief.
    ///
    /// [`crate::Generator::relief`] averages the absolute elevation difference
    /// to the six neighbors and divides by this, so a smaller value makes more
    /// of the world read as steep. Named for its unit: elevation units per one
    /// hex of center-to-center distance.
    pub relief_reference_delta_per_hex: f64,

    /// Macro-region anchor spacing, in hexes.
    pub macro_region_size_hexes: u32,
    /// Region anchor spacing, in hexes.
    pub region_size_hexes: u32,
    /// Chunk edge, in hexes. Chunks serve callers and caches; they do not
    /// define geography, and they are deliberately not an anchored level of the
    /// region hierarchy.
    pub chunk_size_hexes: u32,

    /// Weight of the macro-region level in the combined region parameters of
    /// `DESIGN.md` section 11. Relative to [`Config::region_influence`], not
    /// absolute: the combination divides by the total.
    ///
    /// Named `influence` rather than `weight` because
    /// [`Config::regional_weight`] already names something else entirely — the
    /// share of the *noise* field at regional wavelength in the elevation
    /// composite. The two are independent knobs and a reader must not have to
    /// guess which is which.
    pub macro_region_influence: f64,
    /// Weight of the region level in the combined region parameters.
    pub region_influence: f64,
}

impl Default for Config {
    /// The alpha defaults.
    ///
    /// Wavelengths follow the multi-scale table in `DESIGN.md` section 10, where
    /// one hex of wavelength is six miles of center-to-center distance. These are
    /// starting values for tuning, but changing one changes every world, so a
    /// change here is an algorithm version change like any other.
    fn default() -> Config {
        Config {
            sea_level: 0.0,

            // Measured against `tests/elevation.rs` rather than guessed. Sea
            // level stays at zero, where section 14's scale says it is, and
            // `elevation_offset` is what puts 31 per cent of the world above
            // it. The remaining thresholds then give, as a share of the world:
            // 53 per cent deep water, 16 shallow, 16 lowland, 10 upland, 4
            // highland, and 2 mountain — a shelf that is a fringe of the ocean
            // rather than half of it, and land bands that fall away with
            // height.
            //
            // Re-measured, not re-tuned, when the octave ladder and the
            // sampling offset changed under algorithm version 3: every share
            // moved by a point or two and none of them left the bounds the
            // distribution tests were already asserting, so no threshold
            // followed.
            deep_water_level: -0.15,
            upland_level: 0.18,
            highland_level: 0.35,
            mountain_level: 0.48,

            // 1,000 hexes.
            continental_wavelength_miles: 6_000.0,
            // 300 hexes.
            regional_wavelength_miles: 1_800.0,
            // 20 hexes.
            local_wavelength_miles: 120.0,
            // 6 hexes.
            detail_wavelength_miles: 36.0,

            // 2,000 hexes. Longer than the continental field, so a climate
            // zone is bigger than a continent rather than a stripe across one.
            heat_wavelength_miles: 12_000.0,
            // 1,000 hexes.
            moisture_wavelength_miles: 6_000.0,
            // 100 hexes.
            moisture_variation_wavelength_miles: 600.0,

            // 80 hexes, so crests land roughly 40 hexes apart. That sits inside
            // the 40-200 hex "regional relief" row of the section 10 table.
            ridge_wavelength_miles: 480.0,
            // 40 hexes of directional averaging along the ridge line.
            ridge_elongation_miles: 240.0,

            // 200 hexes, warping by up to 15 hexes.
            warp_wavelength_miles: 1_200.0,
            warp_strength_miles: 90.0,

            // 20 hexes, warping by up to 3 hexes.
            detail_warp_wavelength_miles: 120.0,
            detail_warp_strength_miles: 18.0,

            // Per field. The finest octave of each ladder, at lacunarity 2:
            // continental 375 miles, regional 112.5, ridge 30, local 15,
            // detail 18 — every one of them at or above the twelve-mile
            // Nyquist wavelength of the tile grid.
            //
            // Only the two short fields moved, and they had to: at five octaves
            // each they ran down to 7.5 and 2.25 miles, which the grid cannot
            // carry. Those octaves aliased into per-tile speckle instead, and
            // relief — a first difference between neighbors, which is exactly
            // the operation that amplifies near-Nyquist content — was the
            // visible casualty. The three long fields are inside the bound
            // already and are left alone: widening them would be an
            // unrequested change to the character of the coarse geography
            // wearing a correctness fix as a disguise.
            continental_octaves: 5,
            regional_octaves: 5,
            ridge_octaves: 5,
            local_octaves: 4,
            detail_octaves: 2,
            // Climate is broad by construction: three octaves take the heat
            // field down to 3,000 miles and the moisture field to 1,500, which
            // is still hundreds of hexes. The exit condition for this phase is
            // coherent zones rather than tile-level speckle, and the octave
            // count is where that is won or lost.
            heat_octaves: 3,
            moisture_octaves: 3,
            moisture_variation_octaves: 3,
            fbm_lacunarity: 2.0,
            fbm_gain: 0.5,

            // A halving ladder: each finer scale contributes half of the one
            // above it, so broad structure dominates and detail textures it.
            continental_weight: 1.0,
            regional_weight: 0.5,
            local_weight: 0.15,
            detail_weight: 0.06,

            // Uplift sits between the two coarse noise scales, because that is
            // where the anchor lattice sits: 512 and 128 hexes are 3,072 and
            // 768 miles. Ridges are weaker again, so a mountain belt shapes a
            // continent rather than inventing one.
            uplift_weight: 0.4,
            ridge_weight: 0.3,

            // The same halving ladder again: the broad field decides the
            // climate of a place and the region bias varies it, rather than the
            // two competing. Moisture's local variation is weaker still — it is
            // texture on a wet or dry region, not a second opinion about which
            // one this is.
            heat_field_weight: 1.0,
            heat_region_weight: 0.5,
            moisture_field_weight: 1.0,
            moisture_region_weight: 0.5,
            moisture_variation_weight: 0.25,

            // Measured against `tests/climate.rs`: with cooling disabled the
            // mountain bands sit wherever the broad field put them, and 0.6
            // moves an elevation of +1 down by a little under two heat bands.
            // That is enough for an icecap on a high range inside a temperate
            // zone and not so much that every mountain in the world is polar.
            elevation_cooling: 0.6,

            // Measured, not guessed. Both composites are weighted averages of
            // zero-mean fields and neither is shaped afterwards, so they
            // cluster near the middle of the scale the way the unshaped
            // elevation composite does; these are the quantiles of the
            // generated distribution, rounded, chosen so that no band is empty
            // and none of them holds most of the world. The shares they produce
            // are roughly 15 / 22 / 27 / 25 / 11 for heat and 9 / 25 / 32 /
            // 24 / 10 for moisture. `tests/climate.rs` asserts bounds around
            // those rather than the numbers themselves, so an ordinary retune
            // does not have to touch the test.
            //
            // The heat ladder is the moisture ladder shifted down by 0.05,
            // because elevation cooling only ever subtracts: it pulls the heat
            // distribution below zero and leaves it there. A symmetric ladder
            // over an asymmetric distribution is how a world ends up a third
            // polar.
            polar_level: -0.35,
            cold_level: -0.15,
            temperate_level: 0.05,
            warm_level: 0.25,

            arid_level: -0.30,
            dry_level: -0.10,
            moderate_level: 0.10,
            humid_level: 0.30,

            // Measured, not guessed: the unshaped composite has its 69th
            // percentile at about +0.13, so sinking the world by that much puts
            // a bit under a third of it above sea level. See
            // `tests/elevation.rs`.
            elevation_offset: -0.13,
            elevation_contrast: 1.0,
            elevation_contrast_passes: 2,

            // Measured: the mean absolute step between neighbors is about
            // 0.0127 and the steepest ground reaches 0.047, so a reference of
            // 0.04 puts typical ground around a third of the way up the relief
            // scale and leaves the top of it for genuinely steep places.
            //
            // Re-measured under algorithm version 3, because dropping the
            // aliased octaves lowers the neighbor-step distribution that this
            // value is a reference against — and it turned out not to lower it
            // much. Those octaves carried a sixteenth and a thirty-second of
            // their ladder's amplitude, and the fbm normalization hands most of
            // that back to the octaves that remain, so the mean step moved by
            // about one per cent and the reference did not have to follow. What
            // did move is the *shape*: relief now correlates 0.74 with its
            // neighbor where it correlated 0.70 before, which is the structure
            // the aliasing was burying.
            relief_reference_delta_per_hex: 0.04,

            macro_region_size_hexes: DEFAULT_MACRO_REGION_SIZE_HEXES,
            region_size_hexes: DEFAULT_REGION_SIZE_HEXES,
            chunk_size_hexes: DEFAULT_CHUNK_SIZE_HEXES,

            // The same halving ladder as the composite weights, and for the
            // same reason: the coarser level sets the character of a continent
            // and the finer one varies it, rather than the two competing.
            macro_region_influence: 1.0,
            region_influence: 0.5,
        }
    }
}

impl Config {
    /// Rejects any configuration that cannot produce a well-defined world.
    ///
    /// Checks run in a fixed field order so the reported failure is
    /// deterministic. `NaN` and infinities are rejected here, which is what
    /// keeps them out of the configuration fingerprint.
    pub fn validate(&self) -> Result<(), ConfigError> {
        // The band ladder, in ascending order. Each threshold is checked for
        // range before it is checked against the one below it, so a not-a-number
        // is reported as `NotFinite` rather than silently failing a comparison.
        in_range("deep_water_level", self.deep_water_level, -1.0, 1.0)?;
        in_range("sea_level", self.sea_level, -1.0, 1.0)?;
        in_range("upland_level", self.upland_level, -1.0, 1.0)?;
        in_range("highland_level", self.highland_level, -1.0, 1.0)?;
        in_range("mountain_level", self.mountain_level, -1.0, 1.0)?;
        ascending(&[
            ("deep_water_level", self.deep_water_level),
            ("sea_level", self.sea_level),
            ("upland_level", self.upland_level),
            ("highland_level", self.highland_level),
            ("mountain_level", self.mountain_level),
        ])?;

        // The two climate ladders, checked the same way and for the same
        // reason. Section 16.1 keeps heat and moisture independent, so they are
        // two ladders rather than one.
        in_range("polar_level", self.polar_level, -1.0, 1.0)?;
        in_range("cold_level", self.cold_level, -1.0, 1.0)?;
        in_range("temperate_level", self.temperate_level, -1.0, 1.0)?;
        in_range("warm_level", self.warm_level, -1.0, 1.0)?;
        ascending(&[
            ("polar_level", self.polar_level),
            ("cold_level", self.cold_level),
            ("temperate_level", self.temperate_level),
            ("warm_level", self.warm_level),
        ])?;

        in_range("arid_level", self.arid_level, -1.0, 1.0)?;
        in_range("dry_level", self.dry_level, -1.0, 1.0)?;
        in_range("moderate_level", self.moderate_level, -1.0, 1.0)?;
        in_range("humid_level", self.humid_level, -1.0, 1.0)?;
        ascending(&[
            ("arid_level", self.arid_level),
            ("dry_level", self.dry_level),
            ("moderate_level", self.moderate_level),
            ("humid_level", self.humid_level),
        ])?;

        positive(
            "continental_wavelength_miles",
            self.continental_wavelength_miles,
        )?;
        positive("regional_wavelength_miles", self.regional_wavelength_miles)?;
        positive("local_wavelength_miles", self.local_wavelength_miles)?;
        positive("detail_wavelength_miles", self.detail_wavelength_miles)?;
        positive("ridge_wavelength_miles", self.ridge_wavelength_miles)?;
        positive("ridge_elongation_miles", self.ridge_elongation_miles)?;
        positive("heat_wavelength_miles", self.heat_wavelength_miles)?;
        positive("moisture_wavelength_miles", self.moisture_wavelength_miles)?;
        positive(
            "moisture_variation_wavelength_miles",
            self.moisture_variation_wavelength_miles,
        )?;

        positive("warp_wavelength_miles", self.warp_wavelength_miles)?;
        positive("warp_strength_miles", self.warp_strength_miles)?;
        positive(
            "detail_warp_wavelength_miles",
            self.detail_warp_wavelength_miles,
        )?;
        positive(
            "detail_warp_strength_miles",
            self.detail_warp_strength_miles,
        )?;

        for (field, octaves) in self.octave_ladders().map(|(f, o, _)| (f, o)) {
            if octaves == 0 || octaves > MAX_FBM_OCTAVES {
                return Err(ConfigError::OctaveCount { field, octaves });
            }
        }
        in_range("fbm_lacunarity", self.fbm_lacunarity, 1.0, 4.0)?;
        positive("fbm_gain", self.fbm_gain)?;
        in_range("fbm_gain", self.fbm_gain, 0.0, 1.0)?;

        // Nyquist, after lacunarity is known to be in range: a ladder is only
        // meaningful once the ratio between its rungs is.
        for (field, octaves, wavelength_miles) in self.octave_ladders() {
            self.check_nyquist(field, octaves, wavelength_miles)?;
        }

        positive("continental_weight", self.continental_weight)?;
        positive("regional_weight", self.regional_weight)?;
        positive("local_weight", self.local_weight)?;
        positive("detail_weight", self.detail_weight)?;
        positive("uplift_weight", self.uplift_weight)?;
        positive("ridge_weight", self.ridge_weight)?;
        positive("heat_field_weight", self.heat_field_weight)?;
        positive("heat_region_weight", self.heat_region_weight)?;
        positive("moisture_field_weight", self.moisture_field_weight)?;
        positive("moisture_region_weight", self.moisture_region_weight)?;
        positive("moisture_variation_weight", self.moisture_variation_weight)?;
        in_range("elevation_cooling", self.elevation_cooling, 0.0, 2.0)?;

        in_range("elevation_offset", self.elevation_offset, -1.0, 1.0)?;
        in_range("elevation_contrast", self.elevation_contrast, 0.0, 1.0)?;
        if self.elevation_contrast_passes > MAX_CONTRAST_PASSES {
            return Err(ConfigError::PassCount(self.elevation_contrast_passes));
        }
        positive(
            "relief_reference_delta_per_hex",
            self.relief_reference_delta_per_hex,
        )?;

        positive_size("macro_region_size_hexes", self.macro_region_size_hexes)?;
        positive_size("region_size_hexes", self.region_size_hexes)?;
        positive_size("chunk_size_hexes", self.chunk_size_hexes)?;

        positive("macro_region_influence", self.macro_region_influence)?;
        positive("region_influence", self.region_influence)?;

        Ok(())
    }

    /// The five fbm ladders, as `(octave count field name, count, base
    /// wavelength)`.
    ///
    /// One list, walked by both octave-count checks, so a sixth ladder cannot
    /// be added to the configuration and validated by only one of them.
    fn octave_ladders(&self) -> impl Iterator<Item = (&'static str, u8, f64)> {
        [
            (
                "continental_octaves",
                self.continental_octaves,
                self.continental_wavelength_miles,
            ),
            (
                "regional_octaves",
                self.regional_octaves,
                self.regional_wavelength_miles,
            ),
            (
                "ridge_octaves",
                self.ridge_octaves,
                self.ridge_wavelength_miles,
            ),
            (
                "local_octaves",
                self.local_octaves,
                self.local_wavelength_miles,
            ),
            (
                "detail_octaves",
                self.detail_octaves,
                self.detail_wavelength_miles,
            ),
            (
                "heat_octaves",
                self.heat_octaves,
                self.heat_wavelength_miles,
            ),
            (
                "moisture_octaves",
                self.moisture_octaves,
                self.moisture_wavelength_miles,
            ),
            (
                "moisture_variation_octaves",
                self.moisture_variation_octaves,
                self.moisture_variation_wavelength_miles,
            ),
        ]
        .into_iter()
    }

    /// Rejects a ladder that reaches below what the tile grid can carry.
    ///
    /// The frequency is walked by repeated multiplication, exactly as the fbm
    /// loop in `field.rs` walks it, so the check covers the frequencies that
    /// will actually be sampled rather than a `powf` approximation of them —
    /// and `powf` is barred from the generation path anyway (section 25.2).
    ///
    /// The comparison is `wavelength < limit * frequency` rather than
    /// `wavelength / frequency < limit`: same test, one operation, and no
    /// division of a validated scale by a value that a lacunarity of one leaves
    /// at exactly one.
    fn check_nyquist(
        &self,
        field: &'static str,
        octaves: u8,
        wavelength_miles: f64,
    ) -> Result<(), ConfigError> {
        let limit = crate::NYQUIST_WAVELENGTH_MILES;
        let mut frequency = 1.0_f64;
        for _ in 0..octaves {
            if wavelength_miles < limit * frequency {
                return Err(ConfigError::BelowNyquist {
                    field,
                    octaves,
                    finest_wavelength_miles: wavelength_miles / frequency,
                    limit_miles: limit,
                });
            }
            frequency *= self.fbm_lacunarity;
        }
        Ok(())
    }
}

/// Rejects a ladder of thresholds that does not ascend strictly.
///
/// Shared by the elevation ladder and the two climate ladders. A ladder with an
/// out-of-order rung leaves a band unreachable, and a classifier that can never
/// return a variant is a silently broken world rather than a compile error.
/// Every value is checked for range before it reaches here, so a not-a-number
/// is reported as `NotFinite` rather than slipping through a comparison that is
/// false either way.
fn ascending(ladder: &[(&'static str, f64)]) -> Result<(), ConfigError> {
    for pair in ladder.windows(2) {
        let (below, limit) = pair[0];
        let (field, value) = pair[1];
        if value <= limit {
            return Err(ConfigError::NotAscending {
                field,
                value,
                below,
                limit,
            });
        }
    }
    Ok(())
}

/// Rejects a non-finite or non-positive scale.
fn positive(field: &'static str, value: f64) -> Result<(), ConfigError> {
    if !value.is_finite() {
        return Err(ConfigError::NotFinite { field, value });
    }
    if value <= 0.0 {
        return Err(ConfigError::NotPositive { field, value });
    }
    Ok(())
}

/// Rejects a non-finite value or one outside an inclusive range.
fn in_range(field: &'static str, value: f64, lo: f64, hi: f64) -> Result<(), ConfigError> {
    debug_assert!(lo <= hi);
    if !value.is_finite() {
        return Err(ConfigError::NotFinite { field, value });
    }
    if value < lo || value > hi {
        return Err(ConfigError::OutOfRange {
            field,
            value,
            lo,
            hi,
        });
    }
    Ok(())
}

/// Rejects a zero cell size. A size is the divisor in `div_euclid`, and the
/// equivalence of Euclidean and floor division holds only for a positive
/// divisor, which is why sizes are unsigned and zero is an error.
fn positive_size(field: &'static str, value: u32) -> Result<(), ConfigError> {
    if value == 0 {
        return Err(ConfigError::NotPositive {
            field,
            value: f64::from(value),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_configuration_is_valid() {
        assert_eq!(Config::default().validate(), Ok(()));
    }

    #[test]
    fn default_wavelengths_are_ordered_coarse_to_fine() {
        // The multi-scale sum is meaningless if two scales cross over.
        let c = Config::default();
        assert!(c.continental_wavelength_miles > c.regional_wavelength_miles);
        assert!(c.regional_wavelength_miles > c.local_wavelength_miles);
        assert!(c.local_wavelength_miles > c.detail_wavelength_miles);
    }

    #[test]
    fn default_composite_weights_fall_coarse_to_fine() {
        // The composite of section 10 is dominated by broad structure; a finer
        // scale outweighing a coarser one produces noise, not geography.
        let c = Config::default();
        assert!(c.continental_weight > c.regional_weight);
        assert!(c.regional_weight > c.local_weight);
        assert!(c.local_weight > c.detail_weight);
    }

    #[test]
    fn the_detail_warp_is_shorter_and_weaker_than_the_geographic_warp() {
        // Section 13: low-frequency warps for large geography, weaker
        // high-frequency warps for local irregularity.
        let c = Config::default();
        assert!(c.detail_warp_wavelength_miles < c.warp_wavelength_miles);
        assert!(c.detail_warp_strength_miles < c.warp_strength_miles);
    }

    #[test]
    fn default_scales_are_whole_numbers_of_hexes() {
        // One hex of wavelength is 2 * APOTHEM_MILES of center-to-center
        // distance. DESIGN.md section 10.
        let hex = 2.0 * crate::APOTHEM_MILES;
        let c = Config::default();
        for (field, value) in [
            ("continental", c.continental_wavelength_miles),
            ("regional", c.regional_wavelength_miles),
            ("local", c.local_wavelength_miles),
            ("detail", c.detail_wavelength_miles),
            ("ridge", c.ridge_wavelength_miles),
            ("heat", c.heat_wavelength_miles),
            ("moisture", c.moisture_wavelength_miles),
            ("moisture variation", c.moisture_variation_wavelength_miles),
            ("ridge elongation", c.ridge_elongation_miles),
            ("warp", c.warp_wavelength_miles),
            ("detail warp", c.detail_warp_wavelength_miles),
        ] {
            assert_eq!(
                value % hex,
                0.0,
                "{field} wavelength {value} is not a whole hex count"
            );
        }
    }

    /// A field name paired with a setter for it.
    type FloatSetter = (&'static str, fn(&mut Config, f64));

    /// Every float field, so a new one cannot skip validation unnoticed.
    fn float_fields() -> Vec<FloatSetter> {
        vec![
            ("sea_level", |c, v| c.sea_level = v),
            ("deep_water_level", |c, v| c.deep_water_level = v),
            ("upland_level", |c, v| c.upland_level = v),
            ("highland_level", |c, v| c.highland_level = v),
            ("mountain_level", |c, v| c.mountain_level = v),
            ("continental_wavelength_miles", |c, v| {
                c.continental_wavelength_miles = v
            }),
            ("regional_wavelength_miles", |c, v| {
                c.regional_wavelength_miles = v
            }),
            ("local_wavelength_miles", |c, v| {
                c.local_wavelength_miles = v
            }),
            ("detail_wavelength_miles", |c, v| {
                c.detail_wavelength_miles = v
            }),
            ("ridge_wavelength_miles", |c, v| {
                c.ridge_wavelength_miles = v
            }),
            ("heat_wavelength_miles", |c, v| c.heat_wavelength_miles = v),
            ("moisture_wavelength_miles", |c, v| {
                c.moisture_wavelength_miles = v
            }),
            ("moisture_variation_wavelength_miles", |c, v| {
                c.moisture_variation_wavelength_miles = v
            }),
            ("ridge_elongation_miles", |c, v| {
                c.ridge_elongation_miles = v
            }),
            ("warp_wavelength_miles", |c, v| c.warp_wavelength_miles = v),
            ("warp_strength_miles", |c, v| c.warp_strength_miles = v),
            ("detail_warp_wavelength_miles", |c, v| {
                c.detail_warp_wavelength_miles = v
            }),
            ("detail_warp_strength_miles", |c, v| {
                c.detail_warp_strength_miles = v
            }),
            ("continental_weight", |c, v| c.continental_weight = v),
            ("regional_weight", |c, v| c.regional_weight = v),
            ("local_weight", |c, v| c.local_weight = v),
            ("detail_weight", |c, v| c.detail_weight = v),
            ("uplift_weight", |c, v| c.uplift_weight = v),
            ("ridge_weight", |c, v| c.ridge_weight = v),
            ("heat_field_weight", |c, v| c.heat_field_weight = v),
            ("heat_region_weight", |c, v| c.heat_region_weight = v),
            ("moisture_field_weight", |c, v| c.moisture_field_weight = v),
            ("moisture_region_weight", |c, v| {
                c.moisture_region_weight = v
            }),
            ("moisture_variation_weight", |c, v| {
                c.moisture_variation_weight = v
            }),
            ("elevation_cooling", |c, v| c.elevation_cooling = v),
            ("polar_level", |c, v| c.polar_level = v),
            ("cold_level", |c, v| c.cold_level = v),
            ("temperate_level", |c, v| c.temperate_level = v),
            ("warm_level", |c, v| c.warm_level = v),
            ("arid_level", |c, v| c.arid_level = v),
            ("dry_level", |c, v| c.dry_level = v),
            ("moderate_level", |c, v| c.moderate_level = v),
            ("humid_level", |c, v| c.humid_level = v),
            ("elevation_offset", |c, v| c.elevation_offset = v),
            ("elevation_contrast", |c, v| c.elevation_contrast = v),
            ("relief_reference_delta_per_hex", |c, v| {
                c.relief_reference_delta_per_hex = v
            }),
            ("fbm_lacunarity", |c, v| c.fbm_lacunarity = v),
            ("fbm_gain", |c, v| c.fbm_gain = v),
            ("macro_region_influence", |c, v| {
                c.macro_region_influence = v
            }),
            ("region_influence", |c, v| c.region_influence = v),
        ]
    }

    #[test]
    fn nan_and_infinities_are_rejected_in_every_float_field() {
        for (field, set) in float_fields() {
            for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut c = Config::default();
                set(&mut c, bad);
                let error = c.validate().expect_err("a non-finite value was accepted");
                assert!(
                    matches!(error, ConfigError::NotFinite { field: f, .. } if f == field),
                    "{field} with {bad} produced {error:?}"
                );
            }
        }
    }

    #[test]
    fn non_positive_scales_are_rejected() {
        for field in [
            "continental_wavelength_miles",
            "regional_wavelength_miles",
            "local_wavelength_miles",
            "detail_wavelength_miles",
            "ridge_wavelength_miles",
            "ridge_elongation_miles",
            "warp_wavelength_miles",
            "warp_strength_miles",
            "detail_warp_wavelength_miles",
            "detail_warp_strength_miles",
            "continental_weight",
            "regional_weight",
            "local_weight",
            "detail_weight",
            "uplift_weight",
            "ridge_weight",
            "heat_field_weight",
            "heat_region_weight",
            "moisture_field_weight",
            "moisture_region_weight",
            "moisture_variation_weight",
            "heat_wavelength_miles",
            "moisture_wavelength_miles",
            "moisture_variation_wavelength_miles",
            "relief_reference_delta_per_hex",
            "fbm_gain",
            "macro_region_influence",
            "region_influence",
        ] {
            for bad in [0.0, -0.0, -1.0] {
                let mut c = Config::default();
                let (_, set) = float_fields()
                    .into_iter()
                    .find(|(f, _)| *f == field)
                    .unwrap();
                set(&mut c, bad);
                let error = c.validate().expect_err("accepted a non-positive scale");
                assert!(
                    matches!(error, ConfigError::NotPositive { field: f, .. } if f == field),
                    "{field} with {bad} produced {error:?}"
                );
            }
        }
    }

    #[test]
    fn a_band_threshold_outside_the_normalized_range_is_rejected() {
        for field in [
            "deep_water_level",
            "sea_level",
            "upland_level",
            "highland_level",
            "mountain_level",
        ] {
            for bad in [1.000_001, -1.000_001, 5.0, -5.0] {
                let mut config = Config::default();
                let (_, set) = float_fields()
                    .into_iter()
                    .find(|(name, _)| *name == field)
                    .unwrap();
                set(&mut config, bad);
                let error = config
                    .validate()
                    .expect_err("an out-of-range band threshold was accepted");
                assert!(
                    matches!(
                        error,
                        ConfigError::OutOfRange {
                            field: f,
                            lo: -1.0,
                            hi: 1.0,
                            ..
                        } if f == field
                    ),
                    "{field} with {bad} produced {error:?}"
                );
            }
        }
    }

    #[test]
    fn the_band_ladder_must_ascend_strictly() {
        // Section 14.1's bands are a ladder: an out-of-order threshold would
        // make one band unreachable, and a classifier that can never return a
        // variant is a silently broken world rather than a compile error.
        let ordered = [
            "deep_water_level",
            "sea_level",
            "upland_level",
            "highland_level",
            "mountain_level",
        ];
        let baseline = Config::default();
        let values = [
            baseline.deep_water_level,
            baseline.sea_level,
            baseline.upland_level,
            baseline.highland_level,
            baseline.mountain_level,
        ];
        for pair in values.windows(2) {
            assert!(pair[0] < pair[1], "the defaults are not ascending");
        }

        // Push each threshold down onto the one below it, and then past it.
        for index in 1..ordered.len() {
            for offset in [0.0, 0.1] {
                let mut config = Config::default();
                let (_, set) = float_fields()
                    .into_iter()
                    .find(|(name, _)| *name == ordered[index])
                    .unwrap();
                set(&mut config, values[index - 1] - offset);
                let error = config
                    .validate()
                    .expect_err("a non-ascending ladder was accepted");
                assert!(
                    matches!(
                        error,
                        ConfigError::NotAscending { field, below, .. }
                            if field == ordered[index] && below == ordered[index - 1]
                    ),
                    "{} at {} produced {error:?}",
                    ordered[index],
                    values[index - 1] - offset
                );
            }
        }
    }

    #[test]
    fn the_climate_ladders_must_ascend_strictly() {
        // Section 16.1's two ladders, checked the way section 14.1's is. Two
        // separate ladders rather than one, so a threshold from one axis must
        // never be compared against a threshold from the other.
        for ordered in [
            ["polar_level", "cold_level", "temperate_level", "warm_level"],
            ["arid_level", "dry_level", "moderate_level", "humid_level"],
        ] {
            let baseline = Config::default();
            let values: Vec<f64> = ordered
                .iter()
                .map(|name| match *name {
                    "polar_level" => baseline.polar_level,
                    "cold_level" => baseline.cold_level,
                    "temperate_level" => baseline.temperate_level,
                    "warm_level" => baseline.warm_level,
                    "arid_level" => baseline.arid_level,
                    "dry_level" => baseline.dry_level,
                    "moderate_level" => baseline.moderate_level,
                    "humid_level" => baseline.humid_level,
                    other => unreachable!("{other}"),
                })
                .collect();
            for pair in values.windows(2) {
                assert!(pair[0] < pair[1], "the defaults are not ascending");
            }

            // Push each threshold down onto the one below it, and then past it.
            for index in 1..ordered.len() {
                for offset in [0.0, 0.1] {
                    let mut config = Config::default();
                    let (_, set) = float_fields()
                        .into_iter()
                        .find(|(name, _)| *name == ordered[index])
                        .unwrap();
                    set(&mut config, values[index - 1] - offset);
                    let error = config
                        .validate()
                        .expect_err("a non-ascending ladder was accepted");
                    assert!(
                        matches!(
                            error,
                            ConfigError::NotAscending { field, below, .. }
                                if field == ordered[index] && below == ordered[index - 1]
                        ),
                        "{} at {} produced {error:?}",
                        ordered[index],
                        values[index - 1] - offset
                    );
                }
            }
        }
    }

    #[test]
    fn a_climate_threshold_outside_the_normalized_range_is_rejected() {
        for field in [
            "polar_level",
            "cold_level",
            "temperate_level",
            "warm_level",
            "arid_level",
            "dry_level",
            "moderate_level",
            "humid_level",
        ] {
            for bad in [1.000_001, -1.000_001, 5.0, -5.0] {
                let mut config = Config::default();
                let (_, set) = float_fields()
                    .into_iter()
                    .find(|(name, _)| *name == field)
                    .unwrap();
                set(&mut config, bad);
                let error = config
                    .validate()
                    .expect_err("an out-of-range climate threshold was accepted");
                assert!(
                    matches!(
                        error,
                        ConfigError::OutOfRange {
                            field: f,
                            lo: -1.0,
                            hi: 1.0,
                            ..
                        } if f == field
                    ),
                    "{field} with {bad} produced {error:?}"
                );
            }
        }
    }

    #[test]
    fn an_out_of_range_cooling_strength_is_rejected_and_zero_is_not() {
        // Zero is a legal configuration — it is the one the climate tests use
        // to show what cooling is responsible for — so this cannot be a
        // `positive` check.
        assert_eq!(
            Config {
                elevation_cooling: 0.0,
                ..Config::default()
            }
            .validate(),
            Ok(())
        );
        assert_eq!(
            Config {
                elevation_cooling: 2.0,
                ..Config::default()
            }
            .validate(),
            Ok(())
        );
        for bad in [-0.000_001, 2.000_001, 100.0] {
            let config = Config {
                elevation_cooling: bad,
                ..Config::default()
            };
            assert!(
                matches!(
                    config.validate(),
                    Err(ConfigError::OutOfRange {
                        field: "elevation_cooling",
                        ..
                    })
                ),
                "cooling {bad}"
            );
        }
    }

    #[test]
    fn a_sea_level_that_keeps_the_ladder_ordered_is_accepted() {
        // Sea level is the tuning knob for land fraction, so moving it within
        // the ladder must not need any other edit.
        for good in [-0.1, -0.05, 0.0, 0.1, 0.17] {
            let config = Config {
                sea_level: good,
                ..Config::default()
            };
            assert_eq!(config.validate(), Ok(()), "{good}");
        }
    }

    #[test]
    fn out_of_range_fbm_shaping_is_rejected() {
        for bad in [0.5, 4.5] {
            let config = Config {
                fbm_lacunarity: bad,
                ..Config::default()
            };
            assert!(
                matches!(
                    config.validate(),
                    Err(ConfigError::OutOfRange {
                        field: "fbm_lacunarity",
                        ..
                    })
                ),
                "lacunarity {bad}"
            );
        }
        for good in [1.0, 2.0, 4.0] {
            // One octave per ladder, so the accepted range of lacunarity is
            // what this asserts rather than the Nyquist bound it interacts
            // with: at four, the default five-octave ladders reach below the
            // limit, which is a different rejection with its own test.
            let config = Config {
                fbm_lacunarity: good,
                continental_octaves: 1,
                regional_octaves: 1,
                ridge_octaves: 1,
                local_octaves: 1,
                detail_octaves: 1,
                heat_octaves: 1,
                moisture_octaves: 1,
                moisture_variation_octaves: 1,
                ..Config::default()
            };
            assert_eq!(config.validate(), Ok(()), "lacunarity {good}");
        }

        let config = Config {
            fbm_gain: 1.5,
            ..Config::default()
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::OutOfRange {
                field: "fbm_gain",
                ..
            })
        ));
        assert_eq!(
            Config {
                fbm_gain: 1.0,
                ..Config::default()
            }
            .validate(),
            Ok(())
        );
    }

    /// A field name paired with a setter for an octave count.
    type OctaveSetter = (&'static str, fn(&mut Config, u8));

    /// Every octave-count field, paired with a setter, so a sixth ladder cannot
    /// be added to the configuration and skip these checks unnoticed.
    fn octave_fields() -> Vec<OctaveSetter> {
        vec![
            ("continental_octaves", |c, v| c.continental_octaves = v),
            ("regional_octaves", |c, v| c.regional_octaves = v),
            ("ridge_octaves", |c, v| c.ridge_octaves = v),
            ("local_octaves", |c, v| c.local_octaves = v),
            ("detail_octaves", |c, v| c.detail_octaves = v),
            ("heat_octaves", |c, v| c.heat_octaves = v),
            ("moisture_octaves", |c, v| c.moisture_octaves = v),
            ("moisture_variation_octaves", |c, v| {
                c.moisture_variation_octaves = v
            }),
        ]
    }

    #[test]
    fn the_octave_count_is_rejected_outside_one_through_sixteen_in_every_ladder() {
        for (field, set) in octave_fields() {
            for bad in [0, MAX_FBM_OCTAVES + 1, u8::MAX] {
                let mut config = Config::default();
                set(&mut config, bad);
                assert_eq!(
                    config.validate(),
                    Err(ConfigError::OctaveCount {
                        field,
                        octaves: bad
                    }),
                    "{field} at {bad}"
                );
            }
            // One is always legal: a one-octave ladder is the leaf itself, and
            // no leaf is below the limit while its wavelength validates.
            let mut config = Config::default();
            set(&mut config, 1);
            assert_eq!(config.validate(), Ok(()), "{field} at one octave");
        }
    }

    #[test]
    fn no_default_octave_falls_below_the_nyquist_wavelength_of_the_tile_grid() {
        // Tiles are `2 * APOTHEM_MILES` apart, so the shortest feature the grid
        // can carry is twice that. An octave below it cannot be seen as a
        // feature; it aliases into per-tile noise and relief is the visible
        // casualty. Derived from `APOTHEM_MILES` rather than written as 12.0,
        // so it follows the world scale.
        let limit = 4.0 * crate::APOTHEM_MILES;
        assert_eq!(limit, crate::NYQUIST_WAVELENGTH_MILES);

        let config = Config::default();
        for (field, octaves, wavelength) in config.octave_ladders() {
            // The ladder walked independently of `check_nyquist`: repeated
            // division here against its repeated multiplication.
            let mut finest = wavelength;
            for _ in 1..octaves {
                finest /= config.fbm_lacunarity;
            }
            assert!(
                finest >= limit,
                "{field} = {octaves} reaches {finest} miles, below the {limit}-mile limit"
            );
        }
    }

    #[test]
    fn a_ladder_that_reaches_below_the_nyquist_wavelength_is_rejected() {
        // The check exists so that nobody has to *remember* to drop an octave
        // after shortening a wavelength.
        let limit = crate::NYQUIST_WAVELENGTH_MILES;

        // One octave past the default on each of the two shortest ladders.
        let config = Config {
            detail_octaves: 3,
            ..Config::default()
        };
        assert_eq!(
            config.validate(),
            Err(ConfigError::BelowNyquist {
                field: "detail_octaves",
                octaves: 3,
                finest_wavelength_miles: 9.0,
                limit_miles: limit,
            })
        );
        let config = Config {
            local_octaves: 5,
            ..Config::default()
        };
        assert_eq!(
            config.validate(),
            Err(ConfigError::BelowNyquist {
                field: "local_octaves",
                octaves: 5,
                finest_wavelength_miles: 7.5,
                limit_miles: limit,
            })
        );

        // Shortening a wavelength under an unchanged count fails the same way.
        let config = Config {
            detail_wavelength_miles: 18.0,
            ..Config::default()
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::BelowNyquist {
                field: "detail_octaves",
                ..
            })
        ));

        // And so does a wider lacunarity, which shortens every ladder at once:
        // the first one validation reaches is the one it reports.
        let config = Config {
            fbm_lacunarity: 4.0,
            ..Config::default()
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::BelowNyquist {
                field: "regional_octaves",
                ..
            })
        ));
    }

    #[test]
    fn a_ladder_that_stops_exactly_at_the_nyquist_wavelength_is_accepted() {
        // Exactly at the limit is ugly — two samples per period still beats
        // against the grid — but it is not undefined, so validation draws the
        // line at the hard bound and the defaults sit comfortably above it.
        let config = Config {
            detail_wavelength_miles: crate::NYQUIST_WAVELENGTH_MILES * 2.0,
            detail_octaves: 2,
            ..Config::default()
        };
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn a_zero_cell_size_is_rejected() {
        let cases: [(&str, Config); 3] = [
            (
                "macro_region_size_hexes",
                Config {
                    macro_region_size_hexes: 0,
                    ..Config::default()
                },
            ),
            (
                "region_size_hexes",
                Config {
                    region_size_hexes: 0,
                    ..Config::default()
                },
            ),
            (
                "chunk_size_hexes",
                Config {
                    chunk_size_hexes: 0,
                    ..Config::default()
                },
            ),
        ];
        for (field, config) in cases {
            let error = config
                .validate()
                .expect_err("a zero cell size was accepted");
            assert!(
                matches!(error, ConfigError::NotPositive { field: f, .. } if f == field),
                "{field} produced {error:?}"
            );
        }
    }

    #[test]
    fn an_unknown_field_is_rejected_rather_than_ignored() {
        // Section 21.1: an older binary reading a newer world file must reject
        // it. CBOR here because that is the canonical configuration format.
        let mut bytes = Vec::new();
        ciborium::into_writer(&Config::default(), &mut bytes).unwrap();
        let mut value: ciborium::Value = ciborium::from_reader(bytes.as_slice()).unwrap();
        value.as_map_mut().unwrap().push((
            ciborium::Value::Text("future_field_from_a_newer_version".to_string()),
            ciborium::Value::Float(1.0),
        ));

        let mut extended = Vec::new();
        ciborium::into_writer(&value, &mut extended).unwrap();
        let result: Result<Config, _> = ciborium::from_reader(extended.as_slice());
        assert!(result.is_err(), "an unknown field was silently accepted");
    }

    #[test]
    fn a_missing_field_is_rejected_rather_than_defaulted() {
        // Section 21.1: no field affecting generation may carry
        // `#[serde(default)]`, so dropping one must fail to deserialize.
        let mut bytes = Vec::new();
        ciborium::into_writer(&Config::default(), &mut bytes).unwrap();
        let value: ciborium::Value = ciborium::from_reader(bytes.as_slice()).unwrap();
        let entries = value.as_map().unwrap().clone();
        assert!(!entries.is_empty());

        for index in 0..entries.len() {
            let mut reduced = entries.clone();
            let (removed, _) = reduced.remove(index);
            let mut truncated = Vec::new();
            ciborium::into_writer(&ciborium::Value::Map(reduced), &mut truncated).unwrap();
            let result: Result<Config, _> = ciborium::from_reader(truncated.as_slice());
            assert!(result.is_err(), "{removed:?} was silently defaulted");
        }
    }

    #[test]
    fn serialization_round_trips_and_is_byte_stable() {
        let config = Config::default();
        let mut first = Vec::new();
        ciborium::into_writer(&config, &mut first).unwrap();
        let mut second = Vec::new();
        ciborium::into_writer(&config, &mut second).unwrap();
        assert_eq!(first, second, "canonical bytes must not vary between calls");

        let decoded: Config = ciborium::from_reader(first.as_slice()).unwrap();
        assert_eq!(decoded, config);
    }
}
