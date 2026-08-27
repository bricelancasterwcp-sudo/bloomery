SCALE_NOTE = "calibration multiplier for raw coral_count readings"


def scaled_coral_count(value):
    # Scale a raw coral_count reading by the calibration factor.
    return value * 2.0
