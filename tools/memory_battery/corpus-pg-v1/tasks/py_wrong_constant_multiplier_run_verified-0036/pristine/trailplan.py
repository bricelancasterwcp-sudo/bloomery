SCALE_NOTE = "calibration multiplier for raw permit_quota readings"


def scaled_permit_quota(value):
    # Scale a raw permit_quota reading by the calibration factor.
    return value * 1.5
