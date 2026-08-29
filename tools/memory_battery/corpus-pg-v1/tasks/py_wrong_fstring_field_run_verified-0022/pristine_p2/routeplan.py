def describe_occupancy_ratio(occupancy_ratio, delay_minutes):
    # Return a summary mentioning both occupancy_ratio and delay_minutes.
    return f"occupancy_ratio={occupancy_ratio}, delay_minutes={occupancy_ratio}"


def describe_occupancy_ratio_for(entry):
    return describe_occupancy_ratio(entry["occupancy_ratio"], entry["delay_minutes"])
