#!/bin/bash

# Script to fix theater crate Git tracking issues
# Run this from the root of your theater project

echo "Fixing theater crate Git tracking..."

# Step 1: Check current Git status
echo "=== Current Git Status ==="
git status

# Step 2: Check if there are any submodule references
echo -e "\n=== Checking for submodule references ==="
if [ -f .gitmodules ]; then
    echo "Found .gitmodules file:"
    cat .gitmodules
    echo "Removing .gitmodules if it references theater..."
    # Remove any theater submodule references
    git config --file .gitmodules --remove-section submodule.crates/theater 2>/dev/null || true
    git config --file .gitmodules --remove-section submodule.theater 2>/dev/null || true
    # Remove .gitmodules if it's now empty
    if [ ! -s .gitmodules ]; then
        rm .gitmodules
        echo "Removed empty .gitmodules file"
    fi
else
    echo "No .gitmodules file found"
fi

# Step 3: Remove any Git cache references to theater as a submodule
echo -e "\n=== Removing Git cache references ==="
git rm --cached crates/theater 2>/dev/null || echo "No cached theater reference found"

# Step 4: Make sure there's no .git directory in crates/theater
echo -e "\n=== Checking for nested .git directory ==="
if [ -d "crates/theater/.git" ]; then
    echo "Found .git directory in crates/theater - removing it..."
    rm -rf crates/theater/.git
    echo "Removed crates/theater/.git"
else
    echo "No .git directory found in crates/theater (good!)"
fi

# Step 5: Add the theater crate properly to Git
echo -e "\n=== Adding theater crate to Git ==="
git add crates/theater/
git add .

# Step 6: Show the new status
echo -e "\n=== New Git Status ==="
git status

echo -e "\n=== Theater crate files that will be added ==="
git diff --cached --name-only | grep "crates/theater" || echo "No theater files staged (they might already be tracked)"

echo -e "\nDone! The theater crate should now be properly tracked by Git."
echo "Review the changes above, then commit them with:"
echo "  git commit -m 'fix: properly include theater crate in repository'"
