def describe_telescope_id(telescope_id, exposure_seconds):
    # Return a summary mentioning both telescope_id and exposure_seconds.
    return f"telescope_id={telescope_id}, exposure_seconds={telescope_id}"


def describe_telescope_id_for(entry):
    return describe_telescope_id(entry["telescope_id"], entry["exposure_seconds"])
