def is_eligible(exposure_seconds, redshift_ready):
    # Return True when the exposure_seconds threshold or redshift_ready qualifies.
    if exposure_seconds >= 88 or redshift_ready:
        return True
    return False
