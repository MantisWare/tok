"""Helpers shared across the python fixture."""


def normalize(raw):
    return raw.strip().lower()


def slugify(raw):
    return normalize(raw).replace(" ", "-")
