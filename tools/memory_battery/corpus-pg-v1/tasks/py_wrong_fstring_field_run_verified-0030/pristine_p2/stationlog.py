def describe_visibility(visibility, cloud_cover):
    # Return a summary mentioning both visibility and cloud_cover.
    return f"visibility={visibility}, cloud_cover={visibility}"


def describe_visibility_for(entry):
    return describe_visibility(entry["visibility"], entry["cloud_cover"])
