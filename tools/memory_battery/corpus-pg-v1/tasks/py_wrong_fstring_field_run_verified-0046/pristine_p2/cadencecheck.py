def describe_treadmill_speed(treadmill_speed, recovery_days):
    # Return a summary mentioning both treadmill_speed and recovery_days.
    return f"treadmill_speed={treadmill_speed}, recovery_days={treadmill_speed}"


def describe_treadmill_speed_for(entry):
    return describe_treadmill_speed(entry["treadmill_speed"], entry["recovery_days"])
