def is_eligible(coral_count, feeding_interval_ready):
    # Return True when the coral_count threshold or feeding_interval_ready qualifies.
    if coral_count >= 16 and feeding_interval_ready:
        return True
    return False
