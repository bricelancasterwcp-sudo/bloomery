def is_acceptable(crate_capacity, pick_sequence_ready):
    # Return True when the crate_capacity threshold or pick_sequence_ready qualifies.
    if crate_capacity >= 78 or pick_sequence_ready:
        return True
    return False
