#!/bin/bash

cd "$(dirname "$0")"

# ANSI颜色代码
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 使用echo打印黄色文本
echo -e "${YELLOW} █████╗ ██╗  ██╗ █████╗  ██████╗ ██╗${NC}"
echo -e "${YELLOW}██╔══██╗██║ ██╔╝██╔══██╗██╔════╝ ██║${NC}"
echo -e "${YELLOW}███████║█████╔╝ ███████║██║  ███╗██║${NC}"
echo -e "${YELLOW}██╔══██║██╔═██╗ ██╔══██║██║   ██║██║${NC}"
echo -e "${YELLOW}██║  ██║██║  ██╗██║  ██║╚██████╔╝██║${NC}"
echo -e "${YELLOW}╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝ ╚═╝${NC}"

echo "如果你是付費取得此軟體的使用者，請注意本軟體是開源且免費的 https://github.com/shinkuan/Akagi"
echo "你可能被騙了。請退款、檢舉並差評賣家。"

# GitHub API URL for the latest release
apiUrl="https://api.github.com/repos/shinkuan/Akagi/releases/latest"

# Use curl to call the API and store the response
response=$(curl -s $apiUrl)

# Extract the zipball URL from the response
zipballUrl=$(echo "$response" | grep -o '"zipball_url": "[^"]*' | cut -d'"' -f4)

echo "zipballUrl: $zipballUrl"

# Define the path where the zip file will be saved
zipFilePath="akagi.zip"

# Define the directory to extract the contents of the zip file
extractDir="akagi_extracted"

# Use curl to download the file
curl -s -L -o $zipFilePath $zipballUrl

# Extract the zip file
unzip -q $zipFilePath -d $extractDir

# Find the extracted folder (assuming there's only one folder in the zip file)
extractedFolder=$(find $extractDir -mindepth 1 -maxdepth 1 -type d)

# If there's exactly one directory, try to rename it to "Akagi"
if [ $(echo $extractedFolder | wc -l) -eq 1 ]; then
    originalFolderPath=$extractedFolder
    newFolderName="Akagi"
    newFolderPath="$extractDir/$newFolderName"
    
    echo "Original folder path: $originalFolderPath"
    echo "New folder path: $newFolderPath"
    
    # Check if a folder named "Akagi" already exists
    if [ -d $newFolderPath ]; then
        echo "A folder named '$newFolderName' already exists. Trying to remove it."
        rm -rf $newFolderPath
    fi
    
    # Attempt to rename the folder
    if ! mv $originalFolderPath $newFolderPath; then
        echo "Error renaming folder: $?"
        exit 1
    else
        echo "Folder renamed to '$newFolderName'"
    fi
else
    echo "Expected one directory inside the zip, but found $(echo $extractedFolder | wc -l)"
    exit 1
fi

# Define the destination path where the "Akagi" folder will be moved
destinationPath="./Akagi"

# Check if a folder named "Akagi" already exists at the destination
if [ -d "$destinationPath" ]; then
    echo "A folder named 'Akagi' already exists at the destination. Trying to remove it."
    rm -rf "$destinationPath"
fi

# Move the "Akagi" folder from the "akagi_extracted" directory to the destination
if mv "$newFolderPath" "$destinationPath"; then
    echo "Folder 'Akagi' moved to $destinationPath"
else
    echo "Error moving folder: $?"
    exit 1
fi

# Remove the "akagi_extracted" directory
if [ -d "$extractDir" ]; then
    rm -rf "$extractDir"
    echo "'akagi_extracted' folder has been removed."
fi

# Remove the "akagi.zip" file
if [ -f "$zipFilePath" ]; then
    rm -f "$zipFilePath"
    echo "'akagi.zip' file has been removed."
fi

# Get Python version number
PYTHON_VERSION=$(python3 --version 2>&1 | sed 's/Python //g')

echo "Python version:" $PYTHON_VERSION

MIN_VERSION="3.10.0"
MAX_VERSION="3.12.99"

# Compare versions
version_lte() {
    [ "$1" = "$(echo -e "$1\n$2" | sort -V | head -n1)" ]
}

version_gte() {
    [ "$1" = "$(echo -e "$1\n$2" | sort -V | tail -n1)" ]
}

if version_gte $PYTHON_VERSION $MIN_VERSION && version_lte $PYTHON_VERSION $MAX_VERSION; then
    echo "Python version is in the acceptable range (3.10 - 3.12)."
else
    echo "Python version is not in the acceptable range. Please install Python between 3.10 and 3.12."
    exit
fi


# Define the path to the Akagi folder
akagiPath="./Akagi"

# Change directory to the Akagi folder
cd $akagiPath

# Create a virtual environment named 'venv'
python3 -m venv venv
echo "Virtual environment 'venv' created in the Akagi folder."

# Define the path to the activation script based on the OS
venvActivateScript="venv/bin/activate"

# Check if the activation script exists
if [ -f "$venvActivateScript" ]; then
    # Activate the virtual environment
    source $venvActivateScript
    echo "Activated virtual environment 'venv'."
else
    echo "Activation script not found: $venvActivateScript"
    exit
fi

# Define the path to the requirement.txt file
requirementsPath="requirement.txt"

# Check if the requirements.txt file exists
if [ -f "$requirementsPath" ]; then
    # Install packages from the requirements.txt file
    pip install -r "$requirementsPath"
    echo "Packages from 'requirement.txt' have been installed."
else
    echo "requirement.txt not found: $requirementsPath"
    exit 1
fi

# Run Playwright command to install Chromium
playwright install chromium
echo "Chromium has been installed via Playwright."

echo "Operation completed."
