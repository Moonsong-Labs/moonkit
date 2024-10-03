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

    // A way to represent fungible assets in XCM using Location format
    struct AssetLocationInfo {
        Location location;
        uint256 amount;
    }

    // A way to represent fungible assets in XCM using address format
    struct AssetAddressInfo {
        address asset;
        uint256 amount;
    }

    // The values start at `0` and are represented as `uint8`
    enum TransferType {
        Teleport,
        LocalReserve,
        DestinationReserve,
        RemoteReserve
    }

    /// @dev Function to send assets via XCM using transfer_assets() pallet-xcm extrinsic.
    /// @custom:selector 9ea8ada7
    /// @param dest The destination chain.
    /// @param beneficiary The actual account that will receive the tokens on dest.
    /// @param assets The combination (array) of assets to send.
    /// @param feeAssetItem The index of the asset that will be used to pay for fees.
    function transferAssetsLocation(
        Location memory dest,
        Location memory beneficiary,
        AssetLocationInfo[] memory assets,
        uint32 feeAssetItem
    ) external;

    /// @dev Function to send assets via XCM to a 20 byte-like parachain 
    /// using transfer_assets() pallet-xcm extrinsic.
    /// @custom:selector a0aeb5fe
    /// @param paraId The para-id of the destination chain.
    /// @param beneficiary The actual account that will receive the tokens on paraId destination.
    /// @param assets The combination (array) of assets to send.
    /// @param feeAssetItem The index of the asset that will be used to pay for fees.
    function transferAssetsToPara20(
        uint32 paraId,
        address beneficiary,
        AssetAddressInfo[] memory assets,
        uint32 feeAssetItem
    ) external;

    /// @dev Function to send assets via XCM to a 32 byte-like parachain 
    /// using transfer_assets() pallet-xcm extrinsic.
    /// @custom:selector f23032c3
    /// @param paraId The para-id of the destination chain.
    /// @param beneficiary The actual account that will receive the tokens on paraId destination.
    /// @param assets The combination (array) of assets to send.
    /// @param feeAssetItem The index of the asset that will be used to pay for fees.
    function transferAssetsToPara32(
        uint32 paraId,
        bytes32 beneficiary,
        AssetAddressInfo[] memory assets,
        uint32 feeAssetItem
    ) external;

    /// @dev Function to send assets via XCM to the relay chain 
    /// using transfer_assets() pallet-xcm extrinsic.
    /// @custom:selector 6521cc2c
    /// @param beneficiary The actual account that will receive the tokens on the relay chain.
    /// @param assets The combination (array) of assets to send.
    /// @param feeAssetItem The index of the asset that will be used to pay for fees.
    function transferAssetsToRelay(
        bytes32 beneficiary,
        AssetAddressInfo[] memory assets,
        uint32 feeAssetItem
    ) external;

    // No reserves at all
    function transferAssetsUsingTypeAndThenLocation(
        Location memory dest,
        AssetLocationInfo[] memory assets,
        TransferType assetsTransferType,
        uint8 remoteFeesIdIndex,
        TransferType feesTransferType,
        bytes memory customXcmOnDest
    ) external;

    // Reserve for assets or fees (specified through isAssetReserve)
    function transferAssetsUsingTypeAndThenLocation(
        Location memory dest,
        AssetLocationInfo[] memory assets,
        TransferType assetsTransferType,
        uint8 remoteFeesIdIndex,
        TransferType feesTransferType,
        bytes memory customXcmOnDest,
        Location memory assetsOrFeeRemoteReserve,
        bool isAssetsReserve
    ) external;

    // Reserve for both assets and fees
    function transferAssetsUsingTypeAndThenLocation(
        Location memory dest,
        AssetLocationInfo[] memory assets,
        uint8 remoteFeesIdIndex,
        bytes memory customXcmOnDest,
        Location memory assetsRemoteReserve,
        Location memory feesRemoteReserve
    ) external;

    // No reserves at all
    function transferAssetsUsingTypeAndThenAddress(
        Location memory dest,
        AssetAddressInfo[] memory assets,
        TransferType assetsTransferType,
        uint8 remoteFeesIdIndex,
        TransferType feesTransferType,
        bytes memory customXcmOnDest
    ) external;

    // Reserve for assets or fees (specified through isAssetReserve)
    function transferAssetsUsingTypeAndThenAddress(
        Location memory dest,
        AssetAddressInfo[] memory assets,
        TransferType assetsTransferType,
        uint8 remoteFeesIdIndex,
        TransferType feesTransferType,
        bytes memory customXcmOnDest,
        Location memory assetsOrFeeRemoteReserve,
        bool isAssetsReserve
    ) external;

    // Reserve for both assets and fees
    function transferAssetsUsingTypeAndThenAddress(
        Location memory dest,
        AssetAddressInfo[] memory assets,
        uint8 remoteFeesIdIndex,
        bytes memory customXcmOnDest,
        Location memory assetsRemoteReserve,
        Location memory feesRemoteReserve
    ) external;
}