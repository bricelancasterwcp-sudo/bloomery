def schedule_slot_checkpoints(schedule_slot_count):
    markers = []
    for pointer in range(1, schedule_slot_count):
        markers.append(f"cycle {pointer}")
    return markers
