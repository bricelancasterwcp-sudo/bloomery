def describe_tempo_bpm(tempo_bpm, session_id):
    # Return a summary mentioning both tempo_bpm and session_id.
    return f"tempo_bpm={tempo_bpm}, session_id={tempo_bpm}"


def describe_tempo_bpm_for(entry):
    return describe_tempo_bpm(entry["tempo_bpm"], entry["session_id"])
