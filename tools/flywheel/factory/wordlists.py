"""Domain vocabulary the template families draw from.

Ten unrelated themes (weather stations, astronomy, aquariums, hiking,
music production, gardening, warehouse logistics, fitness tracking,
coffee roasting, transit scheduling) plus a handful of shared pools
(person names, product names, fake domains, config-key bases). None of
these words appear in `codec-tasks-v1`'s billing/cart/shipping/inventory/
networking-config/validator-signup/greeting/stats vocabulary — verified
by `test_templates.py`'s disjointness test against
`contamination.GATE_VOCABULARY`.

Every pool here is a tuple, never a `set` — rng.choice/rng.sample over a
`set` is not reproducible across process runs because Python's string
hashing (and therefore `set` iteration order) is randomized per process
by default. Determinism (brief rule 3) depends on this.
"""

from __future__ import annotations

from typing import NamedTuple


class Theme(NamedTuple):
    id: str
    nouns: tuple[str, ...]  # identifier/variable-name components
    file_stems: tuple[str, ...]  # candidate base filenames for this theme


THEMES: tuple[Theme, ...] = (
    Theme(
        "weather",
        (
            "temperature", "humidity", "wind_speed", "dew_point", "pressure",
            "forecast_hour", "rainfall", "snowfall", "visibility",
            "cloud_cover", "uv_index", "station_id", "gust_speed",
            "barometric_trend",
        ),
        ("stationlog", "forecastcalc", "weathercheck", "climatetrend"),
    ),
    Theme(
        "astronomy",
        (
            "orbital_period", "apparent_magnitude", "right_ascension",
            "declination", "parallax", "luminosity", "transit_depth",
            "exposure_seconds", "telescope_id", "observation_night",
            "redshift", "angular_size",
        ),
        ("orbitwatch", "skyledger", "transitscan", "starfield"),
    ),
    Theme(
        "aquarium",
        (
            "tank_volume", "filter_flow", "ph_level", "salinity",
            "coral_count", "feeding_interval", "water_temp", "algae_growth",
            "quarantine_days", "species_count", "nitrate_level",
            "aeration_rate",
        ),
        ("reeflog", "tanktrack", "aquacare", "brinecheck"),
    ),
    Theme(
        "hiking",
        (
            "trail_length", "elevation_gain", "switchback_count",
            "trailhead_id", "ranger_station", "permit_quota",
            "daypack_weight", "water_capacity", "summit_elevation",
            "descent_rate", "campsite_count", "blaze_spacing",
        ),
        ("trailplan", "summitlog", "ridgewalk", "switchbacks"),
    ),
    Theme(
        "music",
        (
            "tempo_bpm", "waveform_gain", "channel_count", "mixdown_level",
            "sample_rate", "loop_length", "track_duration", "reverb_decay",
            "sidechain_ratio", "session_id", "crossfade_ms", "beat_offset",
        ),
        ("mixdesk", "loopcheck", "sessionlog", "tempotrack"),
    ),
    Theme(
        "gardening",
        (
            "seedling_count", "compost_ratio", "irrigation_cycle",
            "greenhouse_temp", "harvest_day", "soil_ph", "mulch_depth",
            "pollinator_visits", "bed_width", "frost_date", "row_spacing",
            "sunlight_hours",
        ),
        ("plotplan", "harvestlog", "bedtracker", "growcheck"),
    ),
    Theme(
        "warehouse",
        (
            "pallet_count", "forklift_id", "loading_bay", "manifest_weight",
            "dock_schedule", "crate_capacity", "conveyor_speed",
            "scan_batch", "aisle_number", "dispatch_window",
            "pick_sequence", "bay_temperature",
        ),
        ("dockplan", "palletlog", "bayreport", "manifestcheck"),
    ),
    Theme(
        "fitness",
        (
            "interval_count", "heartrate_zone", "rep_count", "rest_seconds",
            "treadmill_speed", "session_minutes", "cadence_rpm",
            "recovery_days", "split_time", "athlete_id", "stride_length",
            "effort_level",
        ),
        ("splitlog", "intervalplan", "sessiontrack", "cadencecheck"),
    ),
    Theme(
        "coffee",
        (
            "roast_level", "bean_weight", "grind_size", "extraction_time",
            "water_temp_c", "cup_count", "batch_id", "cupping_score",
            "bloom_seconds", "dose_ratio", "yield_grams", "tamp_pressure",
        ),
        ("roastlog", "brewcheck", "cuppingnotes", "extractiontrack"),
    ),
    Theme(
        "transit",
        (
            "route_id", "stop_count", "headway_minutes", "platform_number",
            "fare_zone", "dwell_seconds", "delay_minutes", "capacity_limit",
            "boarding_count", "schedule_slot", "transfer_window",
            "occupancy_ratio",
        ),
        ("routeplan", "headwaylog", "platformcheck", "dwelltrack"),
    ),
)

PERSON_NAMES: tuple[str, ...] = (
    "Priya", "Kenji", "Sofia", "Wren", "Tobias", "Marisol", "Idris",
    "Naledi", "Freya", "Dashiell", "Amara", "Callum", "Junko", "Esteban",
    "Ingrid", "Zuri", "Rune", "Ottoline", "Nikhil", "Saoirse",
)

PRODUCT_NAMES: tuple[str, ...] = (
    "Cinderwave", "Thistledown", "Norrath Labs", "Palefire",
    "Kestrel Systems", "Driftwood Co", "Amberlyn", "Solder and Sky",
    "Quillfeather", "Marrow Studio", "Glasswing", "Fernbank",
    "Hollowmere", "Sparrow and Vine", "Lanternfish", "Copperwane",
    "Windlass Labs", "Brackenfield", "Ashgrove", "Nightjar Systems",
    "Emberlin", "Foxglove Studio", "Saltmarsh", "Wrenfield",
    "Duskwater", "Tallowmere",
)

FAKE_DOMAIN_BASES: tuple[str, ...] = (
    "northbridge", "fernbank", "candledrift", "palefire", "kestrel",
    "driftwood", "amberlyn", "hollowmere", "thistledown", "marrow",
    "glasswing", "quillfeather",
)
FAKE_DOMAIN_TLDS: tuple[str, ...] = ("example", "internal", "test")

CONFIG_KEY_BASES: tuple[str, ...] = (
    "relay_port", "gateway_host", "sync_timeout_ms", "batch_size",
    "poll_interval_ms", "cache_ttl_seconds", "replica_count",
    "shard_count", "buffer_size_kb", "heartbeat_interval_ms",
    "max_connections", "backoff_base_ms", "snapshot_interval_min",
    "compaction_threshold", "checkpoint_every_n", "lease_duration_s",
)

DOC_URL_PATHS: tuple[str, ...] = (
    "/docs/setup", "/docs/api", "/help/start", "/guide/quickstart",
    "/support/tickets", "/help/troubleshooting", "/docs/faq",
)

MONTH_NAMES: tuple[str, ...] = (
    "January", "February", "March", "April", "May", "June", "July",
    "August", "September", "October", "November", "December",
)

# Structural identifier pools shared across template families (comparison
# holders, loop indices, boolean flags, multiplier labels, dict keys) —
# routed through this module rather than hardcoded per-family so rule 1's
# disjointness test actually covers them too.
VALUE_HOLDER_NAMES: tuple[str, ...] = (
    "leading_value", "top_pick", "chosen_reading", "running_peak",
    "tracked_value", "selected_reading", "standout_value",
    "current_leader", "extreme_value", "notable_reading",
)

INDEX_VAR_NAMES: tuple[str, ...] = (
    "cursor", "pointer", "slot", "position", "marker", "offset_index",
)

FLAG_NAMES: tuple[str, ...] = (
    "is_ready", "is_eligible", "meets_criteria", "qualifies",
    "is_within_range", "is_acceptable", "passes_check", "is_cleared",
)

MULTIPLIER_LABELS: tuple[str, ...] = (
    "adjustment_factor", "scale_factor", "rate_multiplier",
    "conversion_factor", "weighting_factor", "correction_factor",
)

DICT_KEY_POOL: tuple[str, ...] = (
    "primary", "secondary", "fallback", "override", "baseline",
    "candidate", "nominal", "adjusted",
)

PORT_KEY_NAMES: tuple[str, ...] = (
    "service_port", "gateway_port", "relay_port", "bridge_port",
    "ingress_port", "frontend_port",
)

HOST_KEY_NAMES: tuple[str, ...] = (
    "relay_host", "bridge_host", "frontend_host", "gateway_host",
    "origin_host",
)

# Marker verbs for turn 3's find-shaped multi-file families
# (`templates_multifile_python.py`, `templates_multifile_text.py`). The two
# pools are DISJOINT, and that disjointness is what makes every family's
# `find_pattern` unique to its target by construction rather than by luck:
# a pattern is always `<TARGET verb>_<noun>_<family suffix>`, and every
# identifier a sibling file carries starts with a SIBLING verb, so no
# sibling can contain the pattern regardless of which nouns the draw
# happened to pick. (Matching on the noun alone would not be safe -- one
# theme noun can be a prefix of another, e.g. `water_temp`/`water_temp_c`
# -- which is also why every pattern carries a trailing suffix.)
MULTIFILE_TARGET_VERBS: tuple[str, ...] = (
    "resolve", "compute", "evaluate", "derive", "assemble",
)
MULTIFILE_SIBLING_VERBS: tuple[str, ...] = (
    "summarize", "archive", "mirror", "rotate", "digest",
)


def all_wordlist_tokens() -> frozenset[str]:
    """Every single-token word this module contributes to template
    generation, lowercased and split on non-identifier punctuation only
    where the token is genuinely a bag of separate words (multi-word
    product/action names) — compound identifiers (with underscores) are
    kept whole, matching how `contamination.GATE_VOCABULARY` treats
    compound identifiers. Used by `templates.ALL_TEMPLATE_WORDS` for the
    rule-1 disjointness test."""
    tokens: set[str] = set()

    for theme in THEMES:
        tokens.add(theme.id)
        tokens.update(noun.lower() for noun in theme.nouns)
        tokens.update(stem.lower() for stem in theme.file_stems)

    for pool in (PERSON_NAMES, PRODUCT_NAMES, MONTH_NAMES):
        for entry in pool:
            tokens.update(entry.lower().replace(",", " ").split())

    tokens.update(path.lower() for path in DOC_URL_PATHS)
    tokens.update(base.lower() for base in FAKE_DOMAIN_BASES)
    tokens.update(tld.lower() for tld in FAKE_DOMAIN_TLDS)
    tokens.update(key.lower() for key in CONFIG_KEY_BASES)

    for pool in (
        VALUE_HOLDER_NAMES, INDEX_VAR_NAMES, FLAG_NAMES, MULTIPLIER_LABELS,
        DICT_KEY_POOL, PORT_KEY_NAMES, HOST_KEY_NAMES,
        MULTIFILE_TARGET_VERBS, MULTIFILE_SIBLING_VERBS,
    ):
        tokens.update(entry.lower() for entry in pool)

    return frozenset(tokens)
