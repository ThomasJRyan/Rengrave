# UI
- [x] Move the "New", "Open", "Save", and "Save As" buttons to the "File" dropdown menu
- [x] Change the Workbench options from a drop-down list to a modal that gives you a set of buttons to press to choose which project you want to work on. This modal will be available through File->New. Categorize the buttons by type (image generation, text generation, etc)
- [x] Remove the "sample" input from the text workbenches
- [x] The bar at the top of the gcode preview is currently transparent. Let's make it solid instead
    - [x] The buttons, scrollbars, and inputs in this bar are aligned to the top of the panel. Let's align then to the center instead
- [x] Remove the buttons for exporting Cleanup, SVG, DXF, and Export all under the Export section.
- [x] If possible, render the fonts catalog in their own font.
- [x] Create an SVG of the letter R in a square box, make it green, and replace the "R-Engrave CNC G-code generator" title at the top left with it.
- [x] Under the File dropdown in the menu there are a number of options to choode from. Remove them all except for the aformentioned New, Open, Save and Save As.
- [x] Under the Run dropdown in the menu remove the "Refresh Potrace" option
- [x] In the status bar, remove the Artifacts and Vectorizer statuses

# Functionality
- [x] Add the ability to save a project to a .rgrv file and be able to open it up again later. The format can be JSON under the hood if that is easiest. It should be able to be versioned and allow us to open older versions in newer versions of Rengrave.
- [x] Allow importing an SVG directly into the program instead of requiring an image
- [x] Scan the system fonts automatically on starup. It shouldn't be necessary to click a button to view system fonts
- [x] Remove functionality for the Potrace renderer. We will only use our native Rust renderer going forward. 

# Bugs
- [x] Trying to switch from image engrave/v-carve to text engrave/v-carve does not work. Hopefully with the workflow listed under `UI` this will be resolved. 
- [ ] When attempting to open a font or image file, a system dialog appears to allow the user to browse the file system and select a file. This is correct. However, if the user selects "cancel" then Rengrave opens up a modal looking into the working directory. Remove this modal as it actually makes things more confusing.

# Debugging
- [ ] If possible, add in functionality that allows an AI agent to debug the application on my behalf. This include:
    - [ ] Being able to take and review screenshots of the application
    - [ ] Being able to input button presses and simulate actually using the application
    - [ ] Being able to read debug information about the application
    - [ ] Being able to successfully generate G-Code wit the application
    - [ ] Whatever else would make it easier for an agent to debug
