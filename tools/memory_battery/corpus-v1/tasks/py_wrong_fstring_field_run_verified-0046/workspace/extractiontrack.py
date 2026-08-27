def describe_roast_level(roast_level, bloom_seconds):
    # Return a summary mentioning both roast_level and bloom_seconds.
    return f"roast_level={roast_level}, bloom_seconds={roast_level}"


def describe_roast_level_for(entry):
    return describe_roast_level(entry["roast_level"], entry["bloom_seconds"])
