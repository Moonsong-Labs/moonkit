// SPDX-License-Identifier: GPL-3.0-only
pragma solidity >=0.8.3;

/// @author The Moonbeam Team
/// @title XCM precompile Interface
/// @dev The interface that Solidity contracts use to interact with the substrate pallet-xcm.
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

    /// @dev Function to send assets via XCM using transfer_assets() pallet-xcm extrinsic.
    /// @custom:selector 650ef8c7
    /// @param dest The destination chain.
    /// @param beneficiary The actual account that will receive the tokens in dest.
    /// @param assets The combination (array) of assets to send.
    /// @param feeAssetItem The index of the asset that will be used to pay for fees.
    /// @param weight The weight to be used for the whole XCM operation.
    /// (uint64::MAX in refTime means Unlimited weight) 
    function transferAssets(
        Location memory dest,
        Location memory beneficiary,
        Asset[] memory assets,
        uint32 feeAssetItem,
        Weight memory weight
    ) external;
}