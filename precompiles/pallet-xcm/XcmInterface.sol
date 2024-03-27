// SPDX-License-Identifier: GPL-3.0-only
pragma solidity >=0.8.3;

/// @author The Moonbeam Team
/// @title XCM precompile Interface
/// @dev The interface that Solidity contracts use to interact with the substrate pallet-xcm.
/// @custom:address 0x0000000000000000000000000000000000000820
interface XCM {
    // A location is defined by its number of parents and the encoded junctions (interior)
    struct Location {
        uint8 parents;
        bytes[] interior;
    }

    // Support for Weights V2
    struct Weight {
        uint64 refTime;
        uint64 proofSize;
    }

    // A way to represent fungible assets in XCM
    struct Asset {
        Location location;
        uint256 amount;
    }

    /// @dev Function to make use of the transfer_assets() pallet-xcm extrinsic
    /// @custom:selector 650ef8c7
    function transferAssets(
        Location memory dest,
        Location memory beneficiary,
        Asset[] memory assets,
        uint32 feeAssetItem,
        Weight memory weight
    ) external;
}