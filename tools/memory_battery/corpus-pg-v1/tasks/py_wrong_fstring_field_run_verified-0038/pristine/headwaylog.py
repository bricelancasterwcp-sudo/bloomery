def describe_delay_minutes(delay_minutes, schedule_slot):
    # Return a summary mentioning both delay_minutes and schedule_slot.
    return f"delay_minutes={delay_minutes}, schedule_slot={delay_minutes}"


def describe_delay_minutes_for(entry):
    return describe_delay_minutes(entry["delay_minutes"], entry["schedule_slot"])
