# Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass

#=========================================vcredist================================================================

[Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidUsingWriteHost", "", Justification="Install script called at console")]
<#
    .SYNOPSIS
    Install the VcRedist module and all supported VcRedists on the local system.

    .DESCRIPTION
    Installs the VcRedist PowerShell module and installs the default Microsoft Visual C++ Redistributables on the local system.

    .NOTES
    Copyright 2023, Aaron Parker, stealthpuppy.com
#>
[CmdletBinding()]
param (
    [Parameter(Mandatory = $false)]
    [System.String] $Path = "$env:Temp\VcRedist"
)

Write-Host -ForegroundColor Yellow " █████╗ ██╗  ██╗ █████╗  ██████╗ ██╗"
Write-Host -ForegroundColor Yellow "██╔══██╗██║ ██╔╝██╔══██╗██╔════╝ ██║"
Write-Host -ForegroundColor Yellow "███████║█████╔╝ ███████║██║  ███╗██║"
Write-Host -ForegroundColor Yellow "██╔══██║██╔═██╗ ██╔══██║██║   ██║██║"
Write-Host -ForegroundColor Yellow "██║  ██║██║  ██╗██║  ██║╚██████╔╝██║"
Write-Host -ForegroundColor Yellow "╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝ ╚═╝"
                                    
Write-Host "如果你是付費取得此軟體的使用者，請注意本軟體是開源且免費的 https://github.com/shinkuan/Akagi"
Write-Host "你可能被騙了。請退款、檢舉並差評賣家。"

#region Trust the PSGallery for modules
$Repository = "PSGallery"
if (Get-PSRepository | Where-Object { $_.Name -eq $Repository -and $_.InstallationPolicy -ne "Trusted" }) {
    try {
        Write-Host "Trusting the repository: $Repository."
        Install-PackageProvider -Name "NuGet" -MinimumVersion 2.8.5.208 -Force
        Set-PSRepository -Name $Repository -InstallationPolicy "Trusted"
    }
    catch {
        throw $_
    }
}
#region

#region Install the VcRedist module; https://vcredist.com/
$Module = "VcRedist"
Write-Host "Checking whether module is installed: $Module."
$installedModule = Get-Module -Name $Module -ListAvailable -ErrorAction "SilentlyContinue" | `
    Sort-Object -Property @{ Expression = { [System.Version]$_.Version }; Descending = $true } | `
    Select-Object -First 1
$publishedModule = Find-Module -Name $Module -ErrorAction "SilentlyContinue"
if (($null -eq $installedModule) -or ([System.Version]$publishedModule.Version -gt [System.Version]$installedModule.Version)) {
    Write-Host "Installing module: $Module $($publishedModule.Version)."
    $params = @{
        Name               = $Module
        SkipPublisherCheck = $true
        Force              = $true
        ErrorAction        = "Stop"
    }
    Install-Module @params
}
#endregion


#region tasks/install apps
Write-Host "Saving VcRedists to path: $Path."
New-Item -Path $Path -ItemType "Directory" -Force -ErrorAction "SilentlyContinue" > $null

Write-Host "Downloading and installing supported Microsoft Visual C++ Redistributables."
$Redists = Get-VcList | Save-VcRedist -Path $Path | Install-VcRedist -Silent

Write-Host "Installed Visual C++ Redistributables:"
$Redists | Select-Object -Property "Name", "Release", "Architecture", "Version" -Unique
#endregion

#================================================================================================================

# GitHub API URL for the latest release
$apiUrl = "https://api.github.com/repos/shinkuan/Akagi/releases/latest"

# Use Invoke-RestMethod to call the API and store the response
$response = Invoke-RestMethod -Uri $apiUrl

# Extract the zipball URL from the response
$zipballUrl = $response.zipball_url

# Define the path where the zip file will be saved
$zipFilePath = "akagi.zip"

# Define the directory to extract the contents of the zip file
$extractDir = "akagi_extracted"

# Use Invoke-WebRequest to download the file
Invoke-WebRequest -Uri $zipballUrl -OutFile $zipFilePath

# Extract the zip file
Expand-Archive -LiteralPath $zipFilePath -DestinationPath $extractDir -Force

# Find the extracted folder (assuming there's only one folder in the zip file)
$extractedFolder = Get-ChildItem -Path $extractDir -Directory

# If there's exactly one directory, try to rename it to "Akagi"
if ($extractedFolder.Count -eq 1) {
    $originalFolderPath = $extractedFolder.FullName
    $newFolderName = "Akagi"
    $newFolderPath = Join-Path -Path $extractDir -ChildPath $newFolderName
    
    Write-Host "Original folder path: $originalFolderPath"
    Write-Host "New folder path: $newFolderPath"
    
    # Check if a folder named "Akagi" already exists
    if (Test-Path -Path $newFolderPath) {
        Write-Host "A folder named '$newFolderName' already exists. Trying to remove it."
        Remove-Item -Path $newFolderPath -Recurse -Force
    }
    
    # Attempt to rename the folder
    try {
        Rename-Item -Path $originalFolderPath -NewName $newFolderName
        Write-Host "Folder renamed to '$newFolderName'"
    } catch {
        Write-Host "Error renaming folder: $_"
        exit
    }
} else {
    Write-Host "Expected one directory inside the zip, but found $($extractedFolder.Count)"
    exit
}

# Define the destination path where the "Akagi" folder will be moved
$destinationPath = ".\Akagi"

# Check if a folder named "Akagi" already exists at the destination
if (Test-Path -Path $destinationPath) {
    Write-Host "A folder named 'Akagi' already exists at the destination. Trying to remove it."
    Remove-Item -Path $destinationPath -Recurse -Force
}

# Move the "Akagi" folder from the "akagi_extracted" directory to the destination
try {
    Move-Item -Path $newFolderPath -Destination $destinationPath
    Write-Host "Folder 'Akagi' moved to $destinationPath"
} catch {
    Write-Host "Error moving folder: $_"
    exit
}

# Remove the "akagi_extracted" directory
if (Test-Path -Path $extractDir) {
    Remove-Item -Path $extractDir -Recurse -Force
    Write-Host "'akagi_extracted' folder has been removed."
}

# Remove the "akagi.zip" file
if (Test-Path -Path $zipFilePath) {
    Remove-Item -Path $zipFilePath -Force
    Write-Host "'akagi.zip' file has been removed."
}

try {
    # Invoke Python and get the version information
    $pythonVersionOutput = python --version 2>&1
    Write-Host "Python version output: $pythonVersionOutput"

    if ($pythonVersionOutput -match 'Python (\d+\.\d+\.\d+)') {
        # Extract the version number
        $pythonVersion = $matches[1]
        Write-Host "Detected Python version: $pythonVersion"

        # Compare version numbers
        $minVersion = [Version]"3.10.0"
        $maxVersion = [Version]"3.12.9999"
        $currentVersion = [Version]$pythonVersion

        if ($currentVersion -ge $minVersion -and $currentVersion -le $maxVersion) {
            Write-Host "Python version is in the acceptable range (3.10 - 3.12)."
        } else {
            Write-Host "Python version is not in the acceptable range. Please install Python between 3.10 and 3.12."
            exit
        }
    } else {
        Write-Host "Unable to parse Python version. Please ensure Python is correctly installed."
        exit
    }
} catch {
    Write-Host "Python may not be installed. Error: $_"
    exit
}

# Define the path to the Akagi folder
$akagiPath = ".\Akagi"

# Change directory to the Akagi folder
Push-Location -Path $akagiPath

# Create a virtual environment named 'venv'
python -m venv venv
Write-Host "Virtual environment 'venv' created in the Akagi folder."

# Define the path to the activation script based on the OS
$venvActivateScript = ".\venv\Scripts\Activate.ps1"

# Check if the activation script exists
if (Test-Path -Path $venvActivateScript) {
    # Activate the virtual environment
    . $venvActivateScript
    Write-Host "Activated virtual environment 'venv'."
} else {
    Write-Host "Activation script not found: $venvActivateScript"
    exit
}

# Define the path to the requirement.txt file
$requirementsPath = ".\requirement.txt"

# Check if the requirements.txt file exists
if (Test-Path -Path $requirementsPath) {
    # Install packages from the requirements.txt file
    pip install -r $requirementsPath
    Write-Host "Packages from 'requirement.txt' have been installed."
} else {
    Write-Host "requirement.txt not found: $requirementsPath"
    exit
}

Write-Host "Installing mitm certificate..."
$mitmdumpPath = ".\venv\Scripts\mitmdump.exe"
$confdir = ".\mitmconfig"
if (Test-Path -Path $mitmdumpPath) {
    # create confdir if not exist
    if (-not (Test-Path -Path $confdir -PathType Container)){
        New-Item -Path $confdir -ItemType Directory -Force
    }
    # run mitm proxy for 5 seconds and kill it
    Start-Process -FilePath $mitmdumpPath -ArgumentList "--set confdir=$confdir" -NoNewWindow
    Start-Sleep -Seconds 5
    Stop-Process -Name mitmdump -Force
    # install mitm certificate
    $certutilOutput = Invoke-Expression "certutil -addstore Root '$confdir\mitmproxy-ca-cert.cer'"
    Write-Host $certutilOutput
} else {
    Write-Host "mitmdump not found: $mitmdumpPath"
    exit
}


# Run Playwright command to install Chromium
playwright install chromium
Write-Host "Chromium has been installed via Playwright."

Write-Host "Operation completed."
