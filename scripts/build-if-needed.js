/**
 * Builds the native module for the current platform if it can't
 * be imported.
 */
const path = require('path');
const util = require('util');
const exec = util.promisify(require('child_process').exec);

async function main() {
    // Tries to import the native module.
    // If it fails, then a binary will be built
    try {
        const native = require('../native');
        const version = require('../package.json').version;
        if(native.version() !== version) {
            console.log(`Native library version mismatch: ${native.version()} != ${version}`);
            throw Error('Native library is old')
        }
    } catch (e) {
        // Clear a line before output. Prevents extending previous line when installing
        console.log();
        console.log(`No current binary was found for the platform ${process.platform}.`);
        console.log('A binary will now be built for this platform. This may take a while.');
        process.exit(-1);
    }
}
// Calls the async function from a sync context.
// The process will wait for the async code to complete
// before exiting.
main();
