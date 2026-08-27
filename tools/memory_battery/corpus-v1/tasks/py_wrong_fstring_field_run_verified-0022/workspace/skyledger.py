def describe_declination(declination, observation_night):
    # Return a summary mentioning both declination and observation_night.
    return f"declination={declination}, observation_night={declination}"


def describe_declination_for(entry):
    return describe_declination(entry["declination"], entry["observation_night"])
