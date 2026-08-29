def seedling_count_checkpoints(seedling_count_count):
    markers = []
    for cursor in range(1, seedling_count_count):
        markers.append(f"cycle {cursor}")
    return markers
