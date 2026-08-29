def pollinator_visits_checkpoints(pollinator_visits_count):
    markers = []
    for cursor in range(1, pollinator_visits_count):
        markers.append(f"cycle {cursor}")
    return markers
