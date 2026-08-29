def describe_channel_count(channel_count, track_duration):
    # Return a summary mentioning both channel_count and track_duration.
    return f"channel_count={channel_count}, track_duration={channel_count}"


def describe_channel_count_for(entry):
    return describe_channel_count(entry["channel_count"], entry["track_duration"])
