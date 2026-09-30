#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

echo "=================================================="
echo "🚀 Publishing esapay v1.0.3 to PyPI"
echo "=================================================="
echo ""
echo "Enter your PyPI API Token (starts with pypi-...):"
read -s -p "API Token: " PYPI_TOKEN
echo ""

if [ -z "$PYPI_TOKEN" ]; then
  echo "❌ Token cannot be empty. Please generate one at: https://pypi.org/manage/account/token/"
  exit 1
fi

export TWINE_USERNAME="__token__"
export TWINE_PASSWORD="$PYPI_TOKEN"

echo "📦 Uploading esapay v1.0.3 packages to PyPI..."
./.venv/bin/twine upload --non-interactive sdk/python/dist/esapay-1.0.3*

echo ""
echo "✅ SUCCESS! esapay v1.0.3 has been published to PyPI!"
echo "Verify at: https://pypi.org/project/esapay/1.0.3/"
read -p "Press Enter to exit..."
