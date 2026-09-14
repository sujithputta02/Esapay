#!/bin/bash
set -e

echo "🧠 Starting ARC Fluid Reasoner Service for ESA..."
export FLUID_REASONER_PORT=${FLUID_REASONER_PORT:-5005}
python3 -m arc_reasoner.esa_service
