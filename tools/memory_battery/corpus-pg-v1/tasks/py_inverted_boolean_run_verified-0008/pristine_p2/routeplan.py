def is_eligible(schedule_slot, headway_minutes_ready):
    # Return True when the schedule_slot threshold or headway_minutes_ready qualifies.
    if schedule_slot >= 80 and headway_minutes_ready:
        return True
    return False
