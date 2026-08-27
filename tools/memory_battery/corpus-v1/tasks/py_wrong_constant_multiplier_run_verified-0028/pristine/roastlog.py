SCALE_NOTE = "calibration multiplier for raw batch_id readings"


def scaled_batch_id(value):
    # Scale a raw batch_id reading by the calibration factor.
    return value * 1.75
