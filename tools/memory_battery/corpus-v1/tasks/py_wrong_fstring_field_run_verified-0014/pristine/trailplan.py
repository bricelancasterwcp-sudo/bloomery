def describe_summit_elevation(summit_elevation, trail_length):
    # Return a summary mentioning both summit_elevation and trail_length.
    return f"summit_elevation={summit_elevation}, trail_length={summit_elevation}"


def describe_summit_elevation_for(entry):
    return describe_summit_elevation(entry["summit_elevation"], entry["trail_length"])
