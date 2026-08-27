def describe_sample_rate(sample_rate, mixdown_level):
    # Return a summary mentioning both sample_rate and mixdown_level.
    return f"sample_rate={sample_rate}, mixdown_level={sample_rate}"


def describe_sample_rate_for(entry):
    return describe_sample_rate(entry["sample_rate"], entry["mixdown_level"])
