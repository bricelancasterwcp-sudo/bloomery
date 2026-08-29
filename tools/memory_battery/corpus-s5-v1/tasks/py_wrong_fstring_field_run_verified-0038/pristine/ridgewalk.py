def describe_trailhead_id(trailhead_id, elevation_gain):
    # Return a summary mentioning both trailhead_id and elevation_gain.
    return f"trailhead_id={trailhead_id}, elevation_gain={trailhead_id}"


def describe_trailhead_id_for(entry):
    return describe_trailhead_id(entry["trailhead_id"], entry["elevation_gain"])
